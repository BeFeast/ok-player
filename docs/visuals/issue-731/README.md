# Issue #731 — the chrome's icons come out of the binary

The operator photographed the Settings search field drawing a ring with a stub instead of a
magnifier. Measured on the machine that reported it: the GTK icon theme is `MacTahoe`, a
third-party macOS-style set, and it **does** carry `system-search-symbolic` — its own magnifier,
a large ring with a thin handle, with a hardcoded `fill="#f2f2f2"`. Nothing failed to resolve.
The host theme's design simply appeared in our chrome, and at the size the rail uses it reads as
a ring with a stub. Locale is `C`, so RTL mirroring is not involved.

So the fix is not "ship copies of the standard names as a fallback" — the fallback would never be
reached. Measured on GTK 4.22: a path registered with `gtk_icon_theme_add_resource_path` is
consulted only *after* every theme in the selected theme's inheritance chain, so a theme that
carries a name keeps it, whatever the application ships. The chrome now asks for `okp-*` names,
which no icon theme defines, and gets the 44 symbolic SVGs compiled into the binary.

All captures are the light scheme on a 1920×1080 virtual display, Settings opened on Integration,
taken with `scripts/smoke-linux-chrome-icons.sh` — the same harness the CI check uses.

## The report, reproduced and fixed

Both runs have the same third-party icon theme selected the way KDE selects one
(`gtk-icon-theme-name` in `gtk-3.0/settings.ini` and `gtk-4.0/settings.ini`), and that theme
carries every standard name the chrome used to ask for, each drawn as a white square with a dark
centre so a theme win is unmistakable.

- [Search field, before](search-field-before.png) — `origin/main`. The theme's marker sits where
  the magnifier belongs. This is the reported defect with the design substitution made obvious.
- [Search field, after](search-field-after.png) — the same theme, this branch. The magnifier is
  ours.
- [Whole surface, before](settings-hostile-theme-before.png) and
  [after](settings-hostile-theme.png) — 760×560. Before, the rail's page icons, the search field
  and the retention dropdown's arrow are all the theme's markers. After, none of them are.

Under the hostile theme, `origin/main` looked up six standard names —
`dialog-error-symbolic`, `edit-clear-symbolic`, `pan-down-symbolic`, `system-search-symbolic`,
`view-pin-symbolic`, `window-close-symbolic` — and the theme answered every one. This branch looks
up none of them.

## The clean-container case

- [No icon theme at all](settings-no-icon-theme.png) — an icon theme that carries nothing, and
  `XDG_DATA_DIRS` pointed at an empty directory, so `/usr/share/icons` is not reachable and there
  is no `hicolor` to inherit from. Stronger than any container image: the only icons left
  anywhere are GTK's own builtins and ours. All 44 resolve.

## The set

- [The icon set](icon-set.png) — all 44, at 40 px. Fills only on a 16 px grid: GTK's symbolic
  recolour rewrites `fill` and never `stroke`, so a stroked icon would keep its authored colour
  and stop following the foreground. Original work, drawn by
  `scripts/generate-chrome-icons.py`; nothing is derived from `adwaita-icon-theme` or any other
  set, and nothing is a text glyph.
- [Fractional scale](fractional-scale.png) — five icons rendered through GTK's own symbolic
  loader at true device pixel sizes: 16 px, 24 px (1.5×, which is what `baldr` runs) and 32 px,
  magnified 10× with a point filter so every pixel is visible. The 2 px primary weight lands on
  three whole pixels at 1.5×.

## What the gate reads

Under `OKP_DEBUG_INTERACTIONS` the shell resolves its whole inventory at startup and reports the
file behind every name:

```
interaction: chrome-icon-theme name=OkpHostileTheme count=44
interaction: chrome-icon name=okp-system-search-symbolic source=resource:///com/befeast/okplayer/icons/scalable/actions/okp-system-search-symbolic.svg
```

`scripts/smoke-linux-chrome-icons.sh` asserts that the theme under test really was selected, that
every source is a `resource://` URI under our prefix, that no standard name is looked up at all,
that nothing fell through to `image-missing`, and that the surface drew more than one colour.
