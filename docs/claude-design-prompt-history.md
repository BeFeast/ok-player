# OK Player — History View · Claude Design Brief

*You are designing the **History** surface for OK Player — the most *elegant* media player on Windows: native Fluent/Mica, macOS-utility-grade restraint, libmpv under a single video plane, Windows 11 only. It is a **pure player**: History is the player's own watch-ledger — what you opened, where you left off, what you finished — surfaced so you can fall back into it in one click.*

**The single load-bearing guardrail: History is WATCH HISTORY, never a media library or catalog.** No tags, ratings, collections, genres, smart playlists, per-title metadata editing, watch-count stats, streak counters, or "library size" chrome. Those belong to the separate companion app. The PRD's rule decides every ambiguous call: *"Player, not librarian. When in doubt whether a capability belongs here, it probably belongs in the companion app"* (§4.2, §10.3, P1-D15). If a control can't be phrased as "get me back into a file I already opened," it does not ship here.

**Sources of truth / precedence:** PRD (`docs/OK-Player-PRD.md`) → main design brief (`docs/claude-design-prompt.md`) → compact-modes brief (`docs/claude-design-prompt-compact-modes.md`, match its voice/tokens/rigor) → shipped code on `main` → this brief. The PRD specs history's *data, retention, privacy gating, storage, and management controls* but is **deliberately silent on the browse-history UI itself** — there is no History surface in the §14.1 inventory, only a footer "History" link with no spec. That silence is the design space; everything the PRD *does* say is a hard boundary.

**Four pillars (lower number wins on conflict):** 1) elegant design · 2) subtitle UX · 3) chapters-with-thumbnails · 4) screenshots + frame/second nav. History touches **only pillar 1** — so every decision here is settled by *elegance and restraint*, not feature density. When in doubt, cut.

---

## ⚠️ Solve this first — the foundations every screen hangs on

### Foundation 1 — Where History lives (this is the headline DESIGN QUESTION, see end)

The recommended direction below is **History as a second face of the welcome canvas** — a third visibility state of the existing `WelcomeCard`, a sibling to "Continue watching," reached by the footer link that already exists. This is the most on-product choice: it is the cheapest to build, reuses the welcome shell verbatim, adds zero z-order layers or material decisions over video, and **structurally enforces the "re-entry point, not a library" identity** — because History can only appear in the idle surface, it can never become chrome you manage over your video. Design to this as the primary.

But there is one real, un-resolved fork: the canvas approach is **unreachable while a video plays** — you cannot ask "what did I watch last week" mid-playback. A dedicated `HistoryWindow` (cloned from `SettingsWindow`) would lift that ceiling at the cost of being a heavier surface and one more place to go. **This is DESIGN QUESTION 1 — do not silently resolve it.** Design the canvas version fully; note where a window would differ.

The **in-window slide-over panel over video is rejected** as primary: it forces one control to face two substrates (light Mica welcome *and* video), which is a genuine layout-pop/illegibility hazard the other two approaches dodge — and over the welcome screen it duplicates the shelf two inches to its left.

### Foundation 2 — History is the WHOLE store; the shelf is the resumable SUBSET

This is the entire reason History exists as a distinct surface. The "Continue watching" shelf renders only genuinely-resumable files; the filter is explicit at `PlayerView.xaml.cs` (the `LoadRecents` guard):

```
if (rec.Duration <= 0 || rec.Position <= rec.Duration*0.05 || rec.Position >= rec.Duration-30) continue;
```

i.e. >5% watched **and** not within 30s of the end. So the shelf *structurally excludes* finished files (stored at `Position = 0`, `Finished = true`) and barely-started ones. **History is the inverse: it shows everything `HistoryService.Recents(int.MaxValue)` returns — in-progress, finished, and barely-started — and uses `Finished`/`Progress` to render state, not the resumable gate.** If History can't show a finished film, it's just a taller copy of the shelf and shouldn't exist. Make this superset/subset relationship *visible* (list vs poster strip, dated buckets, finished/not-started rows the shelf can never show) or the surface reads as "the same list twice."

### Foundation 3 — Substrate: themed brushes only

This surface sits on the **light Mica welcome shell, never over video.** Use themed brushes — `TextFillColorPrimary/Secondary/TertiaryBrush`, `OkAccentBrush`, `OkStrokeBrush`, `OkSubtleFillStrongBrush`, `CardBackgroundFillColorSecondaryBrush`, `OkTextSecondaryBrush`. **Never** the over-video tokens (`OkSeek*`, `OkOverVideo*`) — they exist only for floating controls on the video plane and read muddy/low-contrast over Mica. Light + Auto(dark), both fully designed (§16.3). This mirrors the substrate rule the compact-modes brief enforces.

Resolve these three before drawing a pixel.

---

## What History is FOR (and how it differs from the shelf)

| | **Continue watching** (shelf — stays as-is) | **History** (this surface) |
|---|---|---|
| Job | Curated resume teaser — "the handful worth picking back up" | The full honest record — "everything I opened, organized by when" |
| Contents | Resumable subset (>5%, not near-end) | The whole store: in-progress **+ finished + barely-started** |
| Form | Horizontal 194px poster strip | Vertical, dense, date-grouped, scannable list |
| Cap | ≤10, eager posters | Hundreds–thousands, virtualized, lazy posters |

History does not replace the shelf and must not compete with it. They are two reads off the **same** recents/resume record (along with the Windows jump-list — a third mirror; keep all three consistent, but History never *depends* on the jump-list, which is nice-to-have only).

---

## The real data per entry (design only for data that exists)

Each entry is a `FileRecord` (`HistoryService.cs`), keyed by full path in a `Dictionary<string,FileRecord>`. **One row per file**, not a session log — re-watching updates one record's `LastOpenedUtc`; there are no duplicate rows for re-watches. Available fields:

- **`Title`** — filename without extension.
- **`Position`** (seconds) — resume point. **Critical quirk: finished files store `Position = 0` by design**, so a completed file looks identical to a never-started one *except* for the `Finished` flag. **Read `Finished`, never infer completion from position.**
- **`Duration`** (seconds).
- **`Finished`** (bool) — the only reliable "watched to the end" signal.
- **`LastOpenedUtc`** (ISO-8601 "o") — recency sort key; lexical ordinal compare is correct.
- **`PosterPath`** — cached poster frame on disk, decoded async (~20% frame) via the existing pipeline.
- `Bookmarks`, `UserChapters` — present but **not** History-row content (user-authored sidecar content; out of scope here).

**Store constraints that bound the design:**
- **Human-readable JSON, no DB** (`%APPDATA%/OkPlayer/history.json`, atomic temp-then-rename, multi-window concurrent). Search/sort/filter must stay client-side, in-memory, cheap — no index, no fuzzy ranking, no query syntax. Do not ask for DB-class features.
- **Local files only** — `IsTrackable` excludes anything with `://`. No URLs/streams ever appear; don't design for them.
- **Retention-bounded** — `PruneOlderThan` runs on launch (`AppSettings.HistoryRetentionDays`, default = keep forever). Finite, but can still be hundreds–low thousands of rows. Don't assume a small list; don't assume unbounded scrollback.
- **Private mode** (`HistoryService.Private`, session-scoped) gates *writes* only — `Record`/`SetPoster` become no-ops; existing rows stay fully readable. Reflect it honestly; never hide the list because private is on.

---

## The UX to design

### Entry point(s)
The footer **"History" link** (`PlayerView.xaml`, currently a "coming soon" toast at `OnHistoryClick`, `PlayerView.xaml.cs`) is the canonical door. It cross-fades `WelcomeVariationA → WelcomeHistory` (~180ms decelerate). In History, the footer's **left slot flips to "‹ Continue watching"** — same toggle, reversed label; the center recording pill and right Settings gear are reused verbatim. A header back-chevron does the same reverse. The footer (hence the link) exists only on `WelcomeVariationA` (recents present), not `WelcomeFirstRun` — **keep it that way**: with zero history there's nothing to show, so don't expose the door. (If the dedicated-window fork wins instead, also add one hotkey + a context-menu entry so History is reachable mid-playback — see DQ1.)

### Layout (canvas takeover — has-history, light)

```
┌────────────────────────────────────────────────────────────────────────────┐
│   ‹  History                                          [ ⌕  Search…        ] │  ← back glyph + 30px title (matches "Continue watching")
│      Everything you've opened · keeping last 90 days                        │  ← 13.5px secondary; read-only retention echo, links to Settings
│      ( All )   In progress   Finished                                       │  ← OPTIONAL segmented filter (ship only if it reads elegant)
│ ─────────────────────────────────────────────────────────────────────────  │
│   TODAY                                                                     │  ← caption: 11px SemiBold, CharacterSpacing 60, OkTextSecondary
│   ┌──────┐  Dune: Part Two                              Today 21:14    ⋯    │
│   │▓▓▓▓▓▓│  Movies › 2024                              ▌▌▌▌▌▌▌░░  18m left  │  ← in-progress: teal fill + tabular "18m left"
│   └──────┘                                                                  │
│   ┌──────┐  Severance — S02E07                          Today 19:02    ⋯    │
│   │▓▓ ✓ ▓│  Shows › Severance                                   ✓ Finished │  ← finished: check chip, no bar, dimmed thumb
│   └──────┘                                                                  │
│   YESTERDAY                                                                 │
│   ┌──────┐  interview-raw-take3.mov                     Yest. 16:40    ⋯    │
│   │▓▓▓▓▓▓│  Footage › June                                       2m in · 4% │  ← barely-started (shelf excludes; History shows)
│   └──────┘                                                                  │
│              … (virtualized; scrolls)                                       │
│ ─────────────────────────────────────────────────────────────────────────  │
│  ‹ Continue watching              ● Recording history                  ⚙    │  ← reused footer; left slot label flipped
└────────────────────────────────────────────────────────────────────────────┘
```

Same 920px centered column as `WelcomeVariationA`; same `TextFillColor*` ramp; same footer Grid. It should look like one designer drew both faces of the idle surface.

### Grouping & ordering
- **Order:** most-recent-first by `LastOpenedUtc` desc — already `Recents()`'s order; no re-sort, **no user-selectable sort** (a sort dropdown is a library affordance).
- **Buckets:** `TODAY · YESTERDAY · EARLIER THIS WEEK · EARLIER`. **Cap at four** — do not bucket by month/year (that drifts toward a catalog). Date grouping is what makes this read as *re-entry history*, not a grid. Must degrade gracefully to flat if it ever feels heavy.

### Search / filter (curated — the place to resist scope creep)
- A single search box, right-aligned (~240px), **shown only when the list exceeds ~one viewport** (below that it's noise). Client-side `Contains` substring over **title + folder name only**, over the already-loaded bounded set, debounce ~150ms. While querying, suppress group headers and show a flat "N results" list; clearing restores grouping. **No** fuzzy ranking, field scopes, operators, regex, or date pickers — the no-DB model and the 3/10 simplicity bar forbid it.
- **Optional** 3-way segmented filter **All · In progress · Finished** (status as a filter, *not* a second grouping — one organizing axis on screen at a time). `In progress` = the resumable predicate; `Finished` = `Finished == true`; default `All`. Ship only if it reads as elegant rather than busy.

### History row anatomy
| Slot | Content | Rule |
|---|---|---|
| **Thumbnail** | 64×36 (16:9), `CornerRadius 6`, gradient placeholder until decode | `PosterPath`; decode on-demand for realized rows only, off the UI thread |
| **Title** | 13px Medium, 1 line, ellipsis | `Title` ?? filename-sans-ext |
| **Source line** | `Folder › Subfolder` (1–2 parent segments), 11.5px tertiary | derived from path; **full path in tooltip + Copy-path only** — full path on every row reads as "file manager" |
| **State (right)** | in-progress → teal mini-fill + tabular **"18m left"**; barely-started → tabular **"2m in · 4%"**; finished → **`✓ Finished`** chip, no bar | driven by `Finished` first, then `Position`/`Duration` |
| **Last-watched** | `Today 21:14` / `Yest. 16:40` / `12 Jun`, **tabular figures** | from `LastOpenedUtc` |
| **`⋯` overflow** | hover/focus-revealed, ≥32px target | actions below |

**Finished vs in-progress is the whole game.** Finished = subtle check chip, no progress fill, thumbnail dimmed (~0.55) but **title stays fully legible**. In-progress = the teal `#FF28B3AA` fill the shelf already uses on the thumbnail's bottom edge. Tabular/monospaced figures for *every* timecode, duration, percentage, and timestamp (§16.5, non-negotiable).

### Per-item actions (`⋯` MenuFlyout + right-click)
Primary interaction is the cheapest one: **row click = Resume** (open at `Position`; if finished/not-started, open from start). Everything else lives in the overflow:
1. **Resume** — only when resumable (hidden/disabled for finished/not-started).
2. **Play from start** — open at 0, ignoring the resume point.
3. **Reveal in Explorer** (`explorer /select,"<path>"`).
4. **Copy path.**
5. **Remove from history** — single-entry delete (needs a new seam, see below).

Keep it to these five. Hover reveals at most the `⋯`; no multi-select bulk bar. **Excluded on purpose:** "Mark as watched/unwatched" (toggling `Finished` is metadata grooming), "Clear this resume point" ("Play from start" already covers the intent), rename, drag-out — all companion-app territory.

### States (full matrix mandatory — §14.1: never ship a surface missing loading + error)
| State | Treatment |
|---|---|
| **Has-history** | grouped virtualized list as mocked |
| **Loading** | skeleton rows (gradient thumb + shimmer title), no spinner over Mica; only if load >~150ms |
| **Error** (history.json unreadable/corrupt) | quiet inline card: "Couldn't read your history just now" + **Retry**; never blank, never a raw stack/toast |
| **Empty — first-run** | not normally reachable; if entered, calm hero tone: "Nothing here yet — files you open show up in History" |
| **Empty — just cleared** | distinct copy: "History cleared," then auto-return to welcome on next idle |
| **Private-mode active** | thin inline banner *"Private mode — new opens aren't being recorded"* + flipped footer pill; **existing rows stay fully shown** |
| **Filtered — no matches** | "No matches" + clear-filter affordance |
| **Finished / in-progress / barely-started rows** | per row anatomy above |

Each renders in **Light and Auto-dark**, both fully designed. Motion: ~120–250ms decelerate-into-place; the welcome↔history cross-fade ~180ms; honor `prefers-reduced-motion` (instant swap, no translate). 8px grid, Win11 rounding (`OkRowCornerRadius` 7px), designed hover/pressed/focus/selected/disabled (keyboard-driven app), ≥32px hit-targets.

### Long-history scale
- **Virtualize from day one** (`ItemsRepeater` / `ListView` virtualization inside the existing scroll column); grouping headers must not break virtualization.
- **Posters lazy + async + capped** — decode only realized rows off the UI thread, cancel on de-realize; cache to `%APPDATA%/OkPlayer/posters/<SHA1>.png`. **Never eager-generate the whole store** (the shelf eager-generates ≤10; History could be thousands — that spins CPU, violates P3-C6).
- **Build the list off the UI thread** — `Recents()` does a synchronous `File.Exists` per entry; calling it with `int.MaxValue` runs that stat over the entire store on the calling thread, a real stall hazard. Enumerate/filter on a background thread, marshal to the dispatcher. (Optionally a `HistoryService.All()` enumerator that skips the existence gate, letting History *dim* missing files with inline Locate/Remove rather than silently hiding them — strictly better than the shelf's hide, and avoids the per-row sync stat.)
- **Re-read (don't hold a handle) on `Changed`** given concurrent atomic writes from other windows.

---

## Non-goals (explicit — companion-app territory)

- No tags, ratings, favorites, collections, genres, smart playlists.
- No per-title metadata editing or renaming; no "Mark as watched/unwatched."
- No watch-count stats, streaks, "watched 3×" counters, "library size," or any badge/counter chrome (P1-D15).
- No folder trees / catalog grid / column views; no user-selectable sort.
- No retention combo or global "Clear" reimplemented here — **management routes to Settings → Integration → PRIVACY** (`SettingsWindow.xaml`), which already has the retention combo, "Clear watch history…", and confirm dialog. The History subtitle echoes retention as read-only language and links there.
- No dependence on the Windows jump-list (nice-to-have only).

---

## Code seams to build on (named anchors)

| Need | Reuse | Where |
|---|---|---|
| Replace the entry-point stub | `OnHistoryClick` (today toasts "coming soon") → welcome-state switch / `HistoryRequested` | `PlayerView.xaml.cs` |
| Canvas + column + scroll + footer | `WelcomeCard` / `WelcomeVariationA` / footer Grid / recording pill | `PlayerView.xaml` |
| Full-store query (un-gated by resumable filter) | `HistoryService.Recents(int)` | `HistoryService.cs` |
| Per-row state fields | `FileRecord` (`Finished`/`Position`/`Duration`/`LastOpenedUtc`/`PosterPath`/`Title`) | `HistoryService.cs` |
| Row VM patterns (poster/gradient/progress) | `RecentEntry` + `PlaylistRow` (state flags, dimming) | `RecentEntry.cs` |
| Compact list-row + section-header precedent | overflow flyout row (`OkSwitcherRowStyle`) + "MORE TO CONTINUE" caption | `PlayerView.xaml` |
| Poster pipeline (async, cached) | `GeneratePostersAsync` + `ThumbnailService` + `SetPoster` | `PlayerView.xaml.cs` |
| Cross-window live refresh | `HistoryService.Changed` (subscribe/unsubscribe like `PlayerView`) | `HistoryService.cs`; `PlayerView.xaml.cs` |
| Privacy/retention copy + actions to route to | `SettingsWindow` PRIVACY section + `OnClearHistory` | `SettingsWindow.xaml`/`.cs` |
| **Separate-window fork precedent** (if DQ1 chooses window) | `SettingsWindow` shell + `MainWindow.OpenSettings` single-instance lifecycle; clone as `HistoryWindow` + `HistoryRequested` event | `SettingsWindow.xaml.cs`; `MainWindow.xaml.cs` |

**New seams this surface requires (flag as engineering work — they don't exist today):**
- **`bool HistoryService.Remove(string path)`** — deletes one record, persists atomically, and **raises `Changed`** so the shelf, jump-list, and any open window resync. Deletion today is only all-or-nothing (`Clear`) or age-based (`PruneOlderThan`). Per-item delete cannot work honestly without it.
- **A `HistoryRow` VM** (sibling of `PlaylistRow`) projecting a `FileRecord` into row form (`Title`, `FolderText`, `LastWatchedText`, `IsResumable`, `Finished`, `Progress`, `TimeLeftOrState`, lazy `Poster`). Reuse `RecentEntry`'s poster/gradient logic; **do not overload** the 194px continue-watching card VM (its `ProgressFillWidth` is hardcoded `*194` — wrong for a full-width row).

---

## DESIGN QUESTIONS for Oleg

1. **Where does History live — canvas takeover (recommended) or dedicated window?** The canvas approach is the most elegant and most on-product (reinforces "re-entry point, not a library," cheapest, reuses the welcome shell), but **it's unreachable while a video plays.** A `HistoryWindow` (clone of `SettingsWindow`) lifts that ceiling at the cost of being a heavier surface plus a discoverability tax. Is "see my history mid-playback" a real need, or is idle-only the right, restrained boundary? *(If window: also approve one hotkey — e.g. `Ctrl+H` — and a context-menu entry, since History today has none in the keymap §15.2 or context menu §2.12.)*

2. **Ship the `All · In progress · Finished` segmented filter, or omit it for launch?** It's genuinely useful at scale but adds a second control band. Date grouping + recency already carry the surface; the filter is the first thing to cut if it reads busy. Include in MVP or defer?

3. **Missing files: show-and-let-remove, or hide?** History is the natural place to surface "you watched this, the file moved" with inline **Locate / Remove** — more honest than the shelf's silent `File.Exists` hide, and it avoids a per-row synchronous stat on load. Worth the extra row state, or hide missing files like the shelf does?

---

## Deliverables (suggested order)

1. **Lock the three Solve-first decisions** (where it lives per DQ1, superset-vs-subset framing baked into row-state rules, themed-Mica substrate) + the `HistoryRow` VM shape.
2. **Design-system extensions** — only what's net-new, each justified against existing `Ok*` names: the History row template, four section-header captions, in-progress/finished/barely-started state treatments, the search field + optional segmented filter, the relative-last-watched format (tabular). Reuse `OkSwitcherRowStyle`, `OkRowCornerRadius` 7px, `RecentEntry`, the welcome shell, the shelf teal, and `App.History` API by name.
3. **Design the has-history row + grouping until it is unmistakably *not* the shelf** (list vs strip, dated buckets, finished/not-started rows).
4. **Mockups per state** (the coverage table) in **Light and Auto-dark**: has-history, search-active, private-active, empty-first-run, empty-cleared, loading skeleton, error, no-matches.
5. **Shared-component sheet** — the row in all states (in-progress / finished / barely-started / missing) with hover / pressed / focus-selected / disabled, the `⋯` MenuFlyout (five items) open, header, reused footer with flipped left-slot label.
6. **Interaction & redline spec** — 8px-grid sizes (64×36 thumb, row height, 7px radius), the welcome↔history cross-fade (~180ms decelerate + reduced-motion fallback), click-vs-`⋯` split, search/filter behavior, virtualization + on-demand poster decode notes, ≥32px hit-targets, full keyboard map.
7. **Engineering seam note** — `HistoryService.Remove(path)` raising `Changed`; optional `All()` enumerator + background list-build; `HistoryRow` VM; `OnHistoryClick` → state switch (or `HistoryRequested` if window); `Changed` subscribe/unsubscribe; "Manage…/⚙ Privacy" deep-link to Settings → Integration → PRIVACY.
8. **One-page rationale** of the load-bearing choices (canvas over window/panel, full-store superset over the shelf's filter, list over poster grid, date buckets capped at four, management routed to Settings).

*Ship what a designer of IINA, Elmedia, or CleanMyMac would ship: a calm dated ledger you fall back into, not a catalog you manage. Refined > feature-dense. Calm > busy. If it ever feels like a library, it's wrong.*
