# Issue #394 — persistent Linux update decisions

This evidence covers the persistent player update surface and the synchronized
Settings → Updates states. The applicable art direction is the existing
1120×680 player toast material, the canonical 760×560 Settings shell, the
Windows Updates card in `SettingsWindow.xaml`, and the About-panel handoff's
card/button/type tokens. The issue defines the new Update/Skip interaction; it
does not name a separate update-specific design board.

## Captures

- [Persistent update — before the old toast timeout](persistent-update-before-timeout.png)
- [Persistent update — after the old toast timeout](persistent-update-after-timeout.png)
- [Settings — available, Light](settings-available-light.png)
- [Settings — available, Auto-dark](settings-available-dark.png)
- [Settings — skipped exact version](settings-skipped.png)
- [Settings — retryable install failure](settings-install-failure.png)
- [Canonical Settings shell reference](../issue-194/reference-settings-shell.png)

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Player geometry | Preserve the 1120×680 player and use the established top-center notification location without blocking the rest of the canvas. | The action card is centered at the existing 58 px top inset. Only its own bounds accept input; the player, titlebar, and welcome/file-open surfaces remain non-modal and interactive. |
| Settings geometry | Canonical 760×560 window, 192 px rail, 568 px content pane, 24/44 px content gutters, 8 px cards. | All four Settings captures remain 760×560. The decision card stays inside the existing 500 px inner column and reuses the page/card rhythm. |
| Spacing | Existing 8 px grid and restrained card density. | Decision surfaces use 12×14 px padding, 10 px title/action rhythm, 8 px action spacing, 8–10 px radii, and the existing 7 px button radius. |
| Type | Existing Settings and toast ramp. | 13 px semibold title, 12 px supporting copy, and 12 px semibold actions. Versions remain explicit in the title/body and use the existing tabular treatment where Settings presents values. |
| Color/material | Settings cards follow Light/Auto-dark; player notifications use the established invariant dark over-content material. | Settings uses the existing light/dark card stroke and text hierarchy. The player card uses the current dark translucent toast substrate and shadow. Teal is limited to the primary action and existing selection state. |
| Iconography | Do not add a new updater icon family. | The surface is text-led. The existing Updates rail glyph remains unchanged; no decorative update icon was invented. |
| Control states | Available exposes **Update** and **Skip this version**; skipped exposes **Install anyway**; installing disables the primary action; failure restores Update and keeps Skip. | Captures cover available, skipped, and failure. Buttons have explicit accessible labels; normal GTK focus order and `:focus-visible` treatment remain active. |
| Persistence | An automatic prompt must survive the old 1.7 s toast timeout; exact skips persist by channel and a newer version remains eligible. | The deterministic smoke recorded zero pixel difference across the update-card crop after an additional 3 s. It clicked Skip, verified `updates.skipped_versions.public`, restarted, and rendered Install anyway. Core tests cover public/candidate isolation and N → N+1. |
| Failure | The verified installer must remain the only apply path, and failure must keep the version retryable. | The smoke invoked Install anyway against a deliberately unverifiable preview package. Settings rendered the checksum refusal and restored Update + Skip for the same version. |

## Deterministic checks

```text
scripts/smoke-linux-update-surface.sh <binary> <out>
scripts/smoke-linux-settings.sh <binary> <out> updates light available
scripts/smoke-linux-settings.sh <binary> <out> updates dark available
scripts/smoke-linux-settings.sh <binary> <out> updates light skipped
scripts/smoke-linux-settings.sh <binary> <out> updates light install-error
```

The update-surface smoke measured:

```text
surface_variance=0.352791
timeout_difference=0
skip_difference=0.247844
failure_difference=0.038912
manual_check_difference=0
```

## Evidence limits

Xvfb proves deterministic GTK composition, timer-independent persistence,
keyboard-focusable native controls, JSON skip persistence, manual-check state,
and retry after a deliberately failed verified-install attempt. It does not
prove a real candidate download, `pkexec` authentication, package-manager UI,
AppImage replacement/restart, compositor focus, or live GNOME/Wayland
accessibility announcements. Installed candidate acceptance remains an operator
gate under `docs/linux-candidate-upgrade-acceptance.md`; this headless evidence
must not be presented as that live acceptance.
