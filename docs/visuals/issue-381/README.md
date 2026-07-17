# Issue 381 — dedicated Updates Settings page

The canonical Settings reference supplies the existing 760px window, 192px
rail, 42px app-owned titlebar, 171×30 search field, 36px rail rows, and
28/44/28/24 content padding. Its Updates content was a single grouped card.
This change preserves that card's hierarchy and controls while moving its sole
ownership to a dedicated page immediately before Advanced.

## Redline accounting

| Surface | Reference | Implementation |
|---|---|---|
| Geometry | 760px Settings width; 192px rail; content scrolls inside the remaining 568px | Exact existing constants retained; the page also remains usable at 760×360 with independent rail/content scrolling |
| Spacing | 10px rail inset; 2px rail rhythm; 24px content left and 44px right gutter; 14×16 card padding | Existing Settings and info-card classes reused without page-specific spacing overrides |
| Type | 12.5px rail labels; quiet 11px uppercase group title; 12–12.5px labels/values | Existing Settings typography classes reused; updater status keeps the existing 12px wrapped treatment |
| Color/material | Light `#f7f7f5`, dark `#1f1f1f`, card/stroke theme variants, live teal selection accent | No new color tokens; light, dark, and High Contrast all use the established Settings selectors |
| Iconography | 16px outline rail glyphs | Updates uses the established software-download metaphor: a 16px downward arrow entering a tray, distinct from the Advanced braces |
| Controls | Current version, channel, feed, install mode, status, automatic checks, primary update action, Open Releases | The existing widgets and callbacks are moved intact; no duplicate control surface or new polling path exists |
| States | Not checked/up to date, available, progress, error; focus/selected/disabled | Deterministic presentation fixtures cover up-to-date, available, checking, and error; the existing canonical updater state still drives production |
| Behavior | Mouse and keyboard focus, contained scrolling, searchable Settings destinations | Mouse selects the rail row; typed search terms such as “automatic checks” expose an Updates result and Enter navigates; `updates` is accepted as an initial-page ID |

## Captures

- [Up to date · Light](updates-up-to-date-light.png)
- [Update available · Dark](updates-available-dark.png)
- [Checking · Light](updates-checking-light.png)
- [Error · High Contrast](updates-error-high-contrast.png)
- [Search result · Light](updates-search-result-light.png)
- [Minimum supported size · Light](updates-minimum-light.png)

These captures are deterministic X11/Xvfb rendering evidence. They prove page
composition, theme variants, focus/search rendering, routing, and contained
scrolling; they do not claim live GNOME compositor or portal behavior, which is
outside this navigation/content-ownership issue.
