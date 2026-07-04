# okp-core ↔ OkPlayer.Core compatibility note

Records every intentional behavior divergence found while porting C# Core modules to
`rust/crates/okp-core` (EPIC #134). The C# suites in `tests/OkPlayer.Tests` are the executable
spec; each Rust module carries golden tests mirroring every C# case. Anything not listed here
behaves identically on both sides.

## SrtDocument → `okp_core::srt` / Lrc + LyricSync → `okp_core::lrc`

- **Time representation.** C# stores times as `TimeSpan` (100 ns ticks; `TimeSpan.FromSeconds`
  truncates at tick precision). The Rust port keeps `f64` seconds end to end. Differences are
  below 10⁻⁷ s, under the 10⁻⁶ tolerance the C# suite asserts with, and the C# `TimeSpan` range
  guards (overflow-minutes stamps, huge `[offset:…]` values) are reproduced on the same
  boundaries, so acceptance/rejection of pathological input is identical.
- **`LyricSync.ActiveIndex` sentinel.** C# returns `-1` for "no active line";
  `okp_core::lrc::active_index` returns `Option<usize>` with `None` for the same cases.
- **Equal-timestamp ordering.** C# sorts timed lines with `List<T>.Sort`, which is unstable —
  the relative order of lines sharing one timestamp is unspecified. The Rust port uses a stable
  sort, so such lines keep document order. This is a deterministic refinement of the C# contract,
  not a conflict.
- **Doubly-pathological stamp + offset.** A stamp near the `TimeSpan` ceiling combined with a
  near-ceiling negative `[offset:…]` makes the C# offset subtraction overflow and throw,
  despite the parser's "never throws" contract (both values pass their individual range guards).
  The Rust port's `f64` arithmetic just yields a large finite time and never panics. Neither
  suite covers this corner; the Rust behavior is the intended one.

## SubtitleLift → `okp_core::subtitle_lift` / SubtitleStyle → `okp_core::subtitle_style`

- **`FromKey(null)` sentinel.** C# takes a nullable `string?`; the Rust `from_key` takes
  `Option<&str>` with `None` for the same case (matching the `Option`-based convention already
  used across `okp-core`). Both fall back to the Default preset.
- **`FromKey` case-insensitivity.** C# compares keys with `StringComparison.OrdinalIgnoreCase`;
  the Rust port uses `eq_ignore_ascii_case`. The two differ only for non-ASCII input, and every
  preset key is ASCII, so matching is identical for any key that can resolve to a preset; all
  other input falls back to Default on both sides.
- **Preset data shape.** C# exposes `IReadOnlyList<KeyValuePair<string, string>>` built at class
  init; Rust exposes the same ordered pairs as `'static` slices. Keys, values, ordering, and the
  invariant that every preset writes the same six options are identical (pinned by the ported
  suite).

## Playlist → `okp_core::playlist`

- **Item model.** C# `Playlist` holds path strings; the Rust port holds `PlaylistItem` (a local
  path or a stream URL) — the item model of the Linux shell's queue engine, which the port
  absorbs (queue insert modes, reorder, removal, wrap-always transport stepping, and the
  auto-advance toggle; none of these exist in the C# module, whose lists are immutable).
  The auto-advance flag defaults to on — the fixed C# behavior — and Repeat=One bypasses it.
  Construction sorts by the full path/URL string with the ported natural comparer, exactly the
  C# sort; Rust's sort is stable where `List<T>.Sort` is not (same refinement noted for Lrc).
- **`CurrentIndex` sentinel.** C# returns `-1` for "no current item"; `current_index()` returns
  `Option<usize>` with `None` for the same case (the `Option` convention used across `okp-core`).
- **Path matching case.** C# `SetCurrent`/`IndexOf` match paths with
  `StringComparison.OrdinalIgnoreCase` (Windows filesystems are case-insensitive); the Rust port
  matches by exact item equality — on Linux, paths differing only in case are distinct files, so
  ignoring case could conflate them. A Windows consumer (via `okp-ffi`) must normalize case
  before lookup. The ported ignore-case test asserts exact-case behavior instead.
- **Shuffle RNG.** C# uses `System.Random` (time-seeded, injectable seam for tests); the port
  uses a seedable xorshift64 — the Linux shell's shuffle RNG, with the shell providing clock
  entropy via `reseed`. The spec's shuffle tests assert permutation properties (full coverage,
  current-first), not concrete sequences, so both satisfy them; the Fisher–Yates `% (i + 1)`
  modulo bias is negligible at playlist sizes.
- **`Next()`/`Prev()` with duplicate entries.** C# advances by re-finding the peeked *path*, so
  when an M3U repeats an entry the cursor lands on its first occurrence; the Rust port advances
  to the actual neighbouring position. A deterministic refinement — identical whenever entries
  are unique (always, for folder playlists). `reset` with identical items follows the same rule:
  a cursor already sitting on an occurrence equal to the new current item stays put instead of
  being re-found by equality, and the index-returning peeks (`peek_wrapping_index`,
  `auto_advance_target_index`) let a shell load an item first and commit the cursor by position
  only after the player accepts it.

## SubtitleSyncAligner → `okp_core::subtitle_sync`

- **Null/absent sentinels.** C# `Align` returns `null` (and accepts `null` inputs) for the
  no-result cases; `okp_core::subtitle_sync::align` returns `Option<SubtitleSyncResult>` and takes
  slices, which cannot be null — the empty-slice guard covers the same cases. `Votes` is C# `int`;
  the Rust `votes` is `usize`. Purely representational.
- **Optional parameters.** The C# tuning knobs are optional parameters with defaults
  (`minCueWords = 2`, `minMatch = 0.6`, `binSeconds = 0.25`, `maxOffsetSeconds = 120`); Rust has
  no default arguments, so they live in an `AlignOptions` struct whose `Default` carries the same
  values.
- **Tokenizer Unicode classification.** C# classifies UTF-16 code units with
  `char.IsLetterOrDigit` (letter categories + `Nd`); Rust classifies scalar values with
  `char::is_alphanumeric` (Alphabetic + `Nd`/`Nl`/`No`). They differ only on exotica: astral-plane
  letters (e.g. mathematical alphanumerics) are surrogate pairs in C# and split tokens there but
  are kept in Rust, and number-letter/other-number characters (Roman numerals, `½`) are separators
  in C# but token characters in Rust. Likewise C# lowercases with the simple per-`char`
  `ToLowerInvariant` while Rust applies full Unicode lowercasing (multi-char expansions like
  `İ` → `i̇`). Both sides run the one tokenizer over both the ASR words and the cue text, so
  matching stays self-consistent; only cross-implementation offsets on such scripts could differ,
  and no supported ASR source emits them.

## HistoryFormat → `okp_core::history_format`

- **Timestamp input.** C# takes `DateTime` values; the port takes a minimal civil
  `LocalDateTime` (year/month/day/hour/minute) — everything the buckets and labels read
  (nothing below the minute is ever formatted). Where `DateTime` construction validates, the
  port makes a valid civil date the caller's contract (the shell maps it from a real clock).
- **Invariant names.** C# formats with `CultureInfo.InvariantCulture`; the port hardcodes the
  invariant abbreviated weekday/month tables. Output is identical ("Tue 21:48", "12 Jun").

## RecentsShelf → `okp_core::recents_shelf`

- **Counts are `usize`.** C# `int` inputs admit a negative `available`/`unmeasuredDefault`
  (clamped to 0) and, with absurd negative geometry, a negative fit that `Math.Min` would
  return as-is; the unsigned port pins the documented [0, available] clamp. The
  `unmeasuredDefault = 3` default parameter becomes the `DEFAULT_UNMEASURED` const (Rust has
  no default arguments; the `AlignOptions` pattern).

## NfoMetadata → `okp_core::nfo_metadata`

- **XML engine.** C# parses with `XDocument.Parse` (DTD processing prohibited); the port uses
  `roxmltree` (DTD disabled by default). Both reject non-XML text and DOCTYPE-carrying input;
  XML-conformance exotica beyond the .nfo convention may be judged differently by the two
  parsers, but every suite case behaves identically. Element values concatenate all descendant
  text/CDATA, matching `XElement.Value`.
- **Shapes.** `Parse(string?)` → `parse(Option<&str>)`; the record's `int?`/`string?` fields →
  `Option<i32>`/`Option<String>`.
- **Name matching.** C# matches element local names with `OrdinalIgnoreCase`; the port uses
  `eq_ignore_ascii_case` — the looked-up names are ASCII, so only non-ASCII lookalike tags
  differ, and those match on neither side's conventions (same note as SubtitleStyle). The
  `<premiered>`/`<aired>` year prefix is the first four characters (C# slices UTF-16 units,
  Rust chars) — identical for any digit prefix.

## MpvConfText → `okp_core::mpv_conf_text`

- **Shapes only.** `Parse(string?)` → `parse(Option<&str>)`; the `MpvOption` readonly record
  struct → a plain struct. Parsing and serialisation behave identically.

## LaunchArgs → `okp_core::launch_args`

- **Return shape.** C# returns a tuple whose `int?` Sub/Audio use `-1` as "explicit off"; the
  port returns a `LaunchArgs` struct with `Option<TrackSelection>` (`Off` | `Id(n)`), making
  the off-sentinel a variant instead of a magic value. Null `args` → an empty slice (slices
  cannot be null).
- **Option-name matching.** `OrdinalIgnoreCase` → `eq_ignore_ascii_case`: the option names
  ("resume"/"sub"/"audio") are ASCII, so only non-ASCII case-folding lookalikes (e.g. long s)
  differ — such tokens simply fall through to the unknown-switch rule.
- **Unmatched slash tokens.** C# ignores every `/`-prefixed token that is not a documented
  option as an unknown Windows-style switch; on POSIX `/…` is an absolute path, so the port
  keeps unmatched slash tokens positional (`/home/alice/movie.mkv --resume 90` opens the
  file) while the documented names — `/resume`, `/sub`, `/audio`, inline values included —
  still parse as switches. Unknown `-`-prefixed tokens are ignored on both sides. The ported
  unknown-switch test asserts the dash forms; a stray `/foo` becomes positional in Rust,
  where the caller's URL/exists-on-disk validation filters it.

## ImageLuma → `okp_core::image_luma`

- **Shapes only.** `stride` int → `usize` (a negative stride — floored to the 4-byte minimum
  in C# — is unrepresentable); the `stride = 52` default parameter becomes the
  `DEFAULT_STRIDE` const; `ReadOnlySpan<byte>` → `&[u8]`. Scores are identical.

## AspectResize → `okp_core::aspect_resize`

- **Edge codes.** C# takes the raw Win32 `WMSZ_*` int (any unknown code falls into the corner
  branch); the port takes a `ResizeEdge` enum with the same discriminants, so a bogus code is
  unrepresentable rather than silently treated as a corner. The proposed rect is one
  `(left, top, right, bottom)` tuple in and out rather than four scalars. `Math.Round`'s
  banker's rounding is preserved via `round_ties_even`.

## ChapterMath → `okp_core::chapter_math`

- **Index sentinels.** `CurrentIndex` returns `-1` before the first chapter →
  `Option<usize>` with `None`; `JumpTarget`'s `current` parameter follows (`None` = C# `-1`).
  The `epsilon = 0.25` default parameter becomes the `DEFAULT_EPSILON` const.
- **Sort.** C# sorts with unstable `List<T>.Sort` plus an explicit insertion-order tiebreak;
  the port's stable `sort_by(f64::total_cmp)` yields the same order for every real chapter
  list. Only NaN times (absurd input) would be placed differently (`CompareTo` sorts NaN
  first; `total_cmp` sorts positive NaN last).

## Chapter bookmarks → `okp_core::bookmarks` (Linux parity increment)

Windows leads pillar 3 (chapters editor + bookmarks): a viewer can drop position bookmarks and
author titled `UserChapters`. This increment brings the **bookmark** half to Linux — create and
remove position marks from the current playhead, surfaced in the Chapters side panel alongside
the file's own chapters and on the seek timeline.

- **Ported logic.** `okp_core::bookmarks` is the pure list math behind
  `HistoryService.AddBookmark`/`RemoveBookmark`: `add` dedupes within `ADD_DEDUPE_EPSILON`
  (0.5 s) and keeps the list sorted; `remove` matches within `REMOVE_MATCH_EPSILON` (0.01 s).
  The spec is the `Bookmarks_AddDedupeRemove` case in `HistoryServiceTests.cs`. Storage stays
  behind the shell seam: the GTK `HistoryStore` reads/writes `history::FileEntry::bookmarks` and
  persists the same `history.json` as everything else. Non-finite/negative times are rejected in
  the port — a Linux-side hardening; the shell only ever bookmarks a real playhead, so the C#
  suite never hits it.
- **Same schema field as Windows.** Marks persist in the shared `bookmarks` field (Windows
  `Bookmarks`), so a bookmark authored on either platform round-trips to the other. Only local
  files are bookmarkable (history keys off the file path; streams are not tracked), matching
  Windows `IsTrackable`; creating a mark is suppressed in a private session, matching the Windows
  incognito no-op (removing an existing mark is a deliberate edit and stays allowed).
- **Write-back to embedded chapters is intentionally not offered.** A media file's own container
  chapters stay read-only on Linux — editing them would mean remuxing the file, which the alpha
  does not do. Renaming/retitling *user* chapters (Windows `UserChapters`) is likewise deferred;
  Linux ships the position-bookmark flow now and leaves titled user-chapter editing as the
  remaining Windows-leads gap. Existing chapter navigation, hover preview, thumbnails, and
  timeline chapter ticks are unchanged — bookmarks render in their own panel section and on a
  separate timeline rail (the bottom edge, away from the chapters' top rail).

## TrackTags → `okp_core::track_tags`

- **Shapes only.** Nullable strings → `Option<&str>` in, `Option<String>` out. The `" - "`
  split point is a byte offset where C# uses a UTF-16 index, but both guards ("at least one
  character before and after the separator") are positional, so behavior is identical for all
  inputs.

## NetworkPath → `okp_core::network_path`

- **Probe injection.** C#'s parameterless `IsNetwork(path)` wraps a real `DriveInfo` probe;
  core exposes only the injected-probe form — the platform shell supplies it (Windows:
  `DriveInfo`; a Linux shell would classify from its mount table). `DriveType` is ported
  verbatim from `System.IO.DriveType`.
- **Rooting.** C# defers `IsPathRooted`/`GetPathRoot` to `System.IO.Path`, whose rules change
  per OS — the C# suite's engine-agnostic (Linux) runs never see a drive-letter root. The
  port recognizes the union of both platforms' rooted shapes everywhere: ASCII drive-letter
  roots (`C:`, `C:\`, `C:/`) and separator roots (`\`, `/`). Classification is therefore
  deterministic across OSes — a `Z:\…` path reaches the injected probe even on Linux, where
  C#-on-Linux would return false before probing (a combination the C# suite never covers).
  On Windows the results are identical.

## Shortcut/keybinding model → `okp_core::shortcuts` (Linux shell extraction)

- **No C# counterpart.** This module was not ported from `src/OkPlayer.Core`; it is the
  keybinding model extracted from the Linux GTK shell (`okp-linux-gtk`) under the
  freeze-boundary rule (EPIC #134, B6). The spec is the twelve shortcut tests that moved from
  the shell's test module into the core module, not a C# suite. The Windows app has its own
  shortcut handling; if the two ever converge, this module is the shared home.
- **Key identity.** The shell stored chords as `gdk::Key` keysyms; core stores the canonical,
  case-folded keysym *name* (`space`, `comma`, `Page_Up`, `c`). The platform key namespace is
  injected via the `KeyNames` trait: the portable set (display aliases plus ASCII letters and
  digits) resolves in core, and any other config token is resolved by the shell through
  `gdk::Key::from_name` exactly as before — including its case-sensitivity (`Return` resolves,
  `return` does not).
- **Nameless captured keys.** Previously a captured key with no keysym name was accepted,
  serialized as the bogus token `Unknown`, and then invalidated the whole keybinding config on
  the next parse. A name-based chord cannot represent such a key, so capture now rejects it
  with the same "Press a non-modifier key." message. This is the one intentional behavior
  change of the extraction.

## OSC clock → `okp_core::time_code::format_clock` / subtitle delay → `okp_core::subtitle_delay` (Linux shell extraction)

- **Round-vs-floor resolved in favor of floor.** The shipped Linux OSC clock rounded to the
  nearest second, so for the last half of every second it read one ahead of the Windows clock.
  C# `TimeCode.Format` truncates by explicit decision ("you're 'at' second N until N+1", a
  Greptile-era ruling): a clock must not show a second that has not fully elapsed.
  `format_clock` therefore floors, and the Linux shell now uses it everywhere it formatted
  clock text (OSC elapsed/duration labels, seek-hover bubble, chapter rows, A–B loop toasts).
  This is the one intentional behavior change of the extraction, pinned by
  `format_clock_floors_fractional_seconds`.
- **Clock presentation is per-shell styling.** `format_clock` keeps the Linux zero-padded
  shape (`MM:SS` / `HH:MM:SS`, `00:00` for unloaded or invalid positions); the Windows clock
  renders an unpadded leading field (`M:SS` / `H:MM:SS`, exactly `time_code::format`). Same
  truncation, different padding — the shells intentionally differ in presentation only.
- **Delay entry parsing has no C# counterpart.** The Windows delay flyout edits through a
  numeric NumberBox (whole milliseconds), so free-text parsing exists only on Linux: a bare
  number is milliseconds, `ms`/`s` suffixes pick the unit, values clamp to ±600 s. The spec is
  the shell's own tests, which moved into the module (the B6 shortcuts precedent).
- **Millisecond rounding is ties-to-even.** The delay readouts convert seconds to whole
  milliseconds with `round_ties_even`, matching the C# `Math.Round` banker's rounding behind
  `SubDelayMs` on Windows. The shell previously rounded half away from zero; the two differed
  only at exact half-millisecond delays, unreachable through either shell's own controls.

## Update selection → `okp_core::update_selection` (Linux shell extraction)

- **No C# counterpart.** Windows updates flow through Velopack's static feed (`UpdateFeed`,
  pinned by `UpdateFeedTests.cs`), where Velopack itself compares SemVer versions and picks the
  package; nothing to port. This module is the pure half of the Linux `.deb` self-update flow
  extracted from the GTK shell under the freeze-boundary rule (EPIC #134, B8) and migrated to a
  static feed (#162, symmetric to the Windows #131 feed): the `.deb` static-manifest schema
  (`DebFeed`/`DebFeedPackage`), the natural version comparison, and `select_deb_update_from_feed`,
  which returns the feed's `.deb` when it is strictly newer than the running build. Release
  discovery — which release, which assets — now lives in the feed generator
  (`scripts/build-linux-feed.sh` re-derives the manifest from the newest `linux-v*` release), so
  the module just compares the feed's single version to the running one. Network fetch, checksum
  download/verification (fail-closed via the `SHA256SUMS_ASSET` URL the selection carries),
  staging, and installation stay in the shell.
- **Empty vs failed stays honest.** A failed feed fetch surfaces in the shell as an error
  (`LinuxUpdateStatus::Failed`, "couldn't check"); a feed that is not newer than the running build
  returns `None` (`UpToDate`). `select_deb_update_from_feed` never sees a fetch failure, so the two
  outcomes can never be conflated — the same distinction Velopack keeps on Windows (#162 acceptance).
- **Version compare is release-feed-specific.** `compare_versions` orders by numeric runs with
  a lexicographic tiebreak — it is not `natural_compare` (the ported C# filename comparer),
  which interleaves text segments into the comparison. For the single-scheme version strings
  the feed carries (`0.1.0-linux-alpha.N`) the two agree; the update path keeps its shipped
  comparer verbatim.

## Persistence schemas → `okp_core::settings` / `okp_core::history` (shared schema + migration)

The shared, versioned on-disk schemas both shells converge on (EPIC #134, B9). Unlike the
extractions above, these were never a C# port: two divergent on-disk **dialects** shipped —
the Linux GTK shell's snake_case files and the Windows `OkPlayer.Core` PascalCase files — and
this work designs one **canonical** document that is a superset of both, plus the migration
that reads either dialect. The schema and the migration live in core; **path resolution and
file IO stay in each shell** (the "shell seam": `$XDG_CONFIG_HOME`/`$XDG_STATE_HOME` on Linux,
`%APPDATA%` on Windows). The canonical form is snake_case, sectioned/wrapped, and stamped
`version: 2` (bumped from the Linux alpha `1`). `Settings::load` / `History::load` accept the
canonical form, the Linux alpha dialect, and the Windows dialect, and return `None` for
anything else so the shell falls back to defaults — exactly how both shells already treat a
corrupt file. The Windows shell still writes its own PascalCase files today; it adopts the
canonical schema when the C ABI consumer lands (the "%APPDATA% later" note in the issue), and
migrating now, while user state is ~2 testers, keeps that switch cheap. The Windows C# tree is
untouched by this work; the Windows migration is exercised only by the Rust golden tests.

### Why a version bump, and what "migration" means per dialect

- **Linux alpha (`version: 1`, snake_case, `{ version, playback, audio, video, updates,
  advanced }` for settings; `{ version, files }` for history).** The canonical form is a
  structural superset, so an alpha document deserializes directly; `load` only re-stamps the
  version to `2` and leaves every value untouched (the added sections/fields default to absent).
  A tester's alpha file therefore upgrades in place with no data loss, and the shell's on-disk
  output is byte-identical apart from the version stamp and any newly written field. A document
  whose `version` is `0` or greater than `2` is rejected (fall back to defaults) rather than
  silently down-migrated.
- **Windows (PascalCase; settings flat with `SchemaVersion`, history a bare
  `Dictionary<string, FileRecord>` with no wrapper).** Detected by shape — settings by the
  `SchemaVersion` key (the lowercase `version` marks the native dialect), history by the
  absence of the `{ version, files }` wrapper — then remapped field by field into the canonical
  document.

### Settings field map (Windows → canonical)

| Windows `AppSettings` (PascalCase) | Canonical (`okp_core::settings`) |
|---|---|
| `DefaultVolume` (int) | `playback.volume` (f64) |
| `ResumePlayback` | `playback.resume` |
| `DefaultSpeed` | `playback.default_speed` |
| `SkipStep` | `playback.skip_step_seconds` |
| `HideControlsWhenPaused` | `playback.hide_controls_when_paused` |
| `AudioNormalization` | `audio.normalization` |
| `AudioDevice` (`""` = default) | `audio.device` (absent = default) |
| `HardwareDecoding` (bool) | `video.hwdec` (`auto-safe` / `no`) |
| `SubtitleScale` / `SubtitlePosition` / `SubtitleStyle` | `subtitles.scale` / `.position` / `.style` |
| `Theme` / `AccentSource` | `appearance.theme` / `.accent_source` |
| `AutoCheckUpdates` | `updates.auto_check` |
| `HistoryRetentionDays` | `privacy.history_retention_days` |

- **Hardware decoding is stored as the mpv string, not a bool.** The Linux dialect already
  persists `video.hwdec` as the mpv option (`auto-safe` / `no`); the canonical form keeps that
  encoding, so a Windows `HardwareDecoding` bool maps to `auto-safe` (true) or `no` (false).
- **The default audio device is absent, not `""`.** Windows writes `""` for "device not
  remembered"; the canonical form uses an absent field, matching the Linux `auto` convention
  (`AudioSettings::device == None`).
- **`mpv.conf` is not migrated from Windows settings.** On Windows the raw mpv config is a
  separate `%APPDATA%\OkPlayer\mpv.conf` text file, never a field of `settings.json`, so
  `advanced.mpv_conf` is left absent when migrating a Windows settings document (the shell reads
  that file on its own). `advanced.keybindings` is likewise Linux-only.
- **Linux-only vs Windows-only fields coexist.** The picture adjustments
  (`video.brightness`/`contrast`/`saturation`/`gamma`), `playback.auto_advance`, `repeat`, and
  `shuffle` are Linux-only; the `subtitles`, `appearance`, and `privacy` sections are Windows-only
  today. Each shell reads the subset it understands and carries the rest through untouched on
  save, so the shared schema grows without either side dropping the other's state. The three
  Windows-only sections are `skip_serializing_if`-empty, so a Linux document never writes them.

### History field map (Windows → canonical)

| Windows `FileRecord` (PascalCase) | Canonical (`okp_core::history`) |
|---|---|
| `Position` / `Duration` / `Finished` | `position` / `duration` / `finished` |
| `LastOpenedUtc` (ISO-8601 string) | `updated_at_unix` (i64 Unix seconds) |
| `Title` / `PosterPath` | `title` / `poster_path` |
| `Bookmarks` | `bookmarks` |
| `UserChapters` (`{ Time, Title }`) | `chapters` (`{ time, title }`) |
| `AudioId` / `SubtitleId` (int? sentinel) | `preferences.audio_enabled` + `audio_track_id` / subtitle pair |

- **Timestamps become Unix seconds.** The Linux dialect already stores `updated_at_unix`
  (epoch seconds); the canonical form keeps it. Windows `LastOpenedUtc`
  (`DateTime.UtcNow.ToString("o")`, e.g. `2026-01-01T00:00:00.0000000Z`) is parsed to whole
  UTC seconds — fractional seconds and any zone suffix are dropped, and only the `Z` UTC form
  Windows emits is interpreted. An unparseable stamp folds to the epoch (`0`); real Windows
  files always parse. Core carries a self-contained `days_from_civil` for this (no `chrono`
  dependency), the same civil-day algorithm `history_format` uses.
- **Track-id sentinels fold into the enable/track-id pair.** The Linux dialect records audio
  and subtitle selection as an `Option<bool>` "enabled" flag beside an `Option<i64>` track id;
  Windows records a single `int?` with the convention `null` = unrecorded, `-1` = explicitly
  off, `>= 0` = a track id. Migration maps `null → (None, None)`, a negative id
  `→ (Some(false), None)` ("keep it off"), and any other id `→ (Some(true), Some(id))`. This is
  a best-effort reconciliation of the two per-file models; the secondary-subtitle, subtitle
  delay/scale, and speed preferences have no Windows counterpart and stay absent.
- **`bookmarks` is now written on Linux; the other extras are still preserved untouched.**
  `title`, `poster_path`, and `chapters` (`UserChapters`) are carried through the canonical
  record (so a future Windows consumer keeps them); with `bookmarks` they are
  `skip_serializing_if`-empty, so a file with none serializes exactly as the alpha dialect did.
  Linux now writes `bookmarks` — the side panel's position bookmarks persist here (see
  "Chapter bookmarks → `okp_core::bookmarks`" above). To make that safe, the shell's `record()`
  now refreshes a file's progress fields *in place*, preserving every other field (the stored
  `preferences` as before, plus the shared-schema extras); it previously rebuilt the record from
  `default()`, which silently dropped the extras — harmless only while Linux never wrote them,
  but it would have wiped a bookmark on the next progress save.
- **Path keys are carried verbatim.** History is keyed by the raw media path string on both
  platforms (Windows preserves backslashes and original case). Migration does not rewrite keys;
  a cross-platform consumer normalizes case at lookup time (the same note as `Playlist`).

## Player state machine + command/event contract → `okp_core::player` (new C-ABI seam)

Unlike every entry above, `player` is neither a port of a `src/OkPlayer.Core` module nor a lift
of existing shell logic: it is the new typed command/event/snapshot contract the epic calls C10
(#152, shipped in #175) — the seam a shell, or a future C-ABI consumer through `okp-ffi`, drives
the player through. Both shells today handle playback imperatively against their own engine
wrappers (WinUI over libmpv on Windows, `okp-mpv` on Linux) and neither is wired to this machine
yet, so it diverges from nothing shipped; this entry records the intentional model choices so a
later shell rewire is judged against them. The spec is the module's own Rust unit tests (the
`shortcuts`/`update_selection` precedent). `okp-ffi` (the C-ABI seam, shipped in #175) projects
these types across the boundary rather than re-exporting them unchanged. It flattens each tagged
union into a flat `#[repr(C)]` struct — `OkpCommand` is a `kind` discriminant plus every possible
field — and, because C has no `Option`, re-encodes the core's `Option` sentinels as negative magic
values at the edge: `resume_from < 0` and negative track ids fold back to `None`/off. The reject
enum also gains two C-only variants the core never emits — a `None` "not rejected" sentinel (the
outcome is returned by value) and `InvalidArgument` for null or non-UTF-8 input. That boundary
marshalling is the only thing the seam adds; no domain logic lives there (issue #152).

- **Sentinels are `Option`/enums, never magic values.** The convention noted for `Playlist`,
  `ChapterMath`, and the rest carries through: "no active media"/"unknown" are `Option`
  (`snapshot.source`, `time_pos`, `duration`, a track `id: Option<i64>` where `None` = off), and
  lifecycle/category codes are `#[repr(i32)]` enums (`PlaybackStatus`, `EndReason`, `TrackKind`,
  `SeekMode`, `PlayerErrorKind`, `RejectReason`) with stable discriminants a C consumer casts
  straight through — the `aspect_resize::ResizeEdge` pattern. This "never magic values" rule is the
  *core Rust* contract: the `#[repr(i32)]` enums cross the C ABI unchanged, but the `Option` fields
  are the one exception noted above — `okp-ffi` re-encodes them as negative sentinels at the
  boundary precisely because C cannot express `Option`.
- **Optimistic transitions with request-id correlation.** `apply_command` gates a command against
  the current lifecycle state, applies the optimistic transition locally (flipping paused,
  entering `Opening`), and hands back a monotonic `request_id`; only an `Accepted` command consumes
  an id, so a `Rejected` or `NoOp` never burns one and ids never repeat or go backwards. This
  models the `mpv_command_async` reply userdata the event-driven `okp-mpv` will carry — there is no
  C# equivalent to reconcile.
- **Engine-global vs per-file state.** Volume and speed are engine-global: settable before any
  media loads and preserved across `Open`/`Close`, while per-file state (position, duration,
  tracks, chapters, subtitle delay, end reason, last error) resets on every open. A deliberate
  model decision, not a ported one.
- **Finiteness gating at the boundary.** Commands carrying an `f64` — `Seek`, `SetSubtitleDelay`,
  `SetSpeed`, `SetVolume`, and an `Open` `resume_from` — reject non-finite values (`NaN`/±∞) with
  `RejectReason::NotFinite` before any state changes, and the finiteness guard fires ahead of the
  active-media guard. Pure hardening at the contract edge; neither shell's own controls can emit
  such a value.
- **Events fold in only when media is in flight.** A `Loaded` or `Ended` event while `Idle` is
  ignored, and a stray `Paused` property echo reconciles the lifecycle only while playback is
  active (`Playing`/`Paused`) — so out-of-order engine wake-ups cannot manufacture a phantom
  transition. The `Ended` state keeps the media context current until the next `Open`/`Close`.
