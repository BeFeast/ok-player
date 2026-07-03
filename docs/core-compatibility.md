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
