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
  invariant that every preset writes the same seven options are identical (pinned by the ported
  suite).
- **Linux presentation helpers.** `next`, `normalized_scale`, `normalized_position`, and
  `is_managed_option` are Rust-side extractions for the GTK shell. They encode the same Windows
  defaults/ranges and preset order, plus the advanced-config protection needed when Linux merges
  its raw `mpv.conf` text. They do not alter the shared preset data.
- **libmpv compatibility.** `okp-mpv` applies the shared modern option set directly when
  `sub-border-style` exists. On mpv 0.37 it translates outline presets to a transparent legacy
  background plus `sub-shadow-color`; a non-transparent `sub-back-color` retains the same High
  contrast background-box intent. This engine adapter does not change persisted keys or preset
  semantics.

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

## Poster-frame selection → `okp_core::poster_frame` (Linux Continue Watching/History)

- **Selection policy ported.** `poster_sample_offsets`, `PosterFrameScorer`, and the
  `POSTER_LIT_ENOUGH` / `POSTER_MIN_USABLE_LUMA` thresholds port the Windows
  `PlayerView.PickRepresentativeFrameAsync` + `GeneratePostersAsync` gate: the same
  `{0.15, 0.25, 0.38, 0.50, 0.62, 0.75, 0.82}` sampling spread (each floored to a 3s minimum),
  the same `litEnough = 48` early-stop and `minUsableLuma = 22` usable floor, and the same
  "keep the brightest, one fixed 30s grab when the duration is unknown" behaviour. The shell
  decodes candidate frames and feeds their [`image_luma`] scores in; the pick/verdict is shared.
- **Cache identity is stronger than the Windows poster filename.** Windows names the poster PNG
  by `SHA1(path)` alone and re-validates its luma on every pass; the port's `poster_cache_key`
  hashes path **plus byte length plus mtime** (matching the Windows *thumbnail* file key and the
  Linux hover/chapter fingerprint), so a replaced file derives a new key and its stale poster is
  never served — the invalidation the regression requires. The Linux shell therefore derives the
  poster path from current file identity each pass rather than persisting `poster_path` into
  `history.json`; the shared `FileEntry.poster_path` field stays the Windows carry-through.
- **Durable "no usable frame" verdict.** `PosterVerdict::Unusable` is the Rust form of the
  Windows `NoUsablePoster` sentinel (an all-black film keeps its placeholder and is not
  re-derived); `NoFrame` distinguishes a transient decode miss (retry later) from that durable
  verdict. On Linux the sentinel is an on-disk `<key>.none` marker in the cache rather than a
  string stored in history.
- **Audio cover art is out of scope here.** Windows' poster pass also fills audio recents from
  sidecar/embedded cover art (`EnsureAudioPosterAsync`); the Linux poster path classifies audio
  (and URL/network) sources as a non-video fallback and generates no frame for them. This
  regression is specifically the local-video representative-frame path.

## AspectResize → `okp_core::aspect_resize`

- **Edge codes.** C# takes the raw Win32 `WMSZ_*` int (any unknown code falls into the corner
  branch); the port takes a `ResizeEdge` enum with the same discriminants, so a bogus code is
  unrepresentable rather than silently treated as a corner. The proposed rect is one
  `(left, top, right, bottom)` tuple in and out rather than four scalars. `Math.Round`'s
  banker's rounding is preserved via `round_ties_even`.
- **Linux client-size Shift-resize has no C# counterpart.** Windows drives aspect from `WM_SIZING`
  on an outer rect with explicit non-client insets (`constrain`). Wayland has no client-visible
  window position and GTK4 dropped `GDK_HINT_ASPECT`, so the interactive Shift-resize (issue #331)
  works from logical pointer deltas instead. `AspectResize` projects each app-owned drag update onto
  the locked aspect line (straight dragged axis leads; corners follow the dominant signed fractional
  delta), clamps to the OSC floor and compositor-reported workarea, and returns the opposite-edge
  anchor delta where the platform can apply it. Pressing or releasing Shift rebases the pointer
  origin to the size already reached, so neither transition snaps back. X11 applies both size and
  anchor position; Wayland applies stable size-only requests because normal toplevel positioning is
  deliberately unavailable. The shell never feeds configure sizes back into the state machine, so
  one pointer update produces at most one size request and cannot create a configure-correction loop.
  Real Mutter pointer feel and the accepted Wayland anchoring compromise remain
  `gnome-wayland-operator` acceptance (see `docs/linux-release-acceptance.md`).

## WindowFit → `okp_core::window_fit`

- **Geometry parity.** `fit_to_work_area`, `fill_client_to_work_area`, and `fill_client` port the
  complete C# `WindowFitTests` contract. Rust uses explicit `WindowSize`/`WindowRect` values and
  `Option` for invalid inputs; the C# module uses tuples and nullable tuples. The shared 94% work
  area budget, aspect preservation, no-upscale rule, correction thresholds, and tie-to-even
  rounding are unchanged.
- **Linux initial-fit lifecycle has no C# counterpart.** `InitialFitState` owns the
  one-shot request per media generation, and `initial_fit_can_configure` keeps a deferred first map
  pending until the realized toplevel has compositor-reported bounds. WinUI can address a concrete
  monitor and window position through `AppWindow`; GTK must bootstrap the Wayland surface before
  libmpv can publish dimensions. Waiting for both inputs lets the shell compute one final placement
  instead of visibly retargeting after map. These helpers add platform-neutral lifecycle policy
  around the ported arithmetic without changing any C# test case.

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

## Lyrics sidecar discovery → `okp_core::lyrics` (Linux shell extraction)

The Windows synced-lyrics feature resolves a sheet in four steps — sidecar `.lrc` → metadata-keyed
cache → LRCLIB exact → LRCLIB fuzzy — inside `OkPlayer.App/Services/LyricsService.cs` (App layer,
untested). The Linux slice (issue #189) ports **only the sidecar seam** to the core, where it is
directly unit-tested, and renders it in the GTK shell; cache and network are deferred.

- **Sidecar path rule.** `sidecar_path` mirrors the C# `TryReadSidecar`: same directory, same file
  stem, a lowercase `.lrc` (`track.flac` → `track.lrc`, only the last extension replaced), and
  `None` for any path containing `://` (a stream URL). One divergence: C# also returns null when
  `GetDirectoryName` is empty (a bare, directory-less filename), a quirk the shell never hits
  because its current path is always absolute; the Rust port instead resolves a bare filename to a
  sibling `.lrc`, which is the sensible result and is what the unit tests pin.
- **Case sensitivity.** Windows leans on a case-insensitive `File.Exists`; Linux filesystems are
  case-sensitive, so `read_sidecar` probes the canonical stem with the `.lrc` extension in every
  ASCII-case spelling (`lrc`, `LRC`, `Lrc`, …). A sheet exported as `Track.LRC`, `Track.Lrc`, or any
  mixed case therefore resolves. These are bounded direct reads, never a directory scan — a scan
  could stall on a slow network mount and, worse, would run on every track with no sidecar at all
  (the common case). Only the extension case is folded, not the stem: a Windows `File.Exists` also
  matches a differently-cased *stem* (media `Song.flac` ↔ sidecar `song.lrc`), but export tools
  always derive the sidecar name from the audio file, so its stem matches by construction, and
  case-folding the stem would require the very directory scan this seam avoids.
- **Never fails.** Any I/O error (an unmounted share, a permission error) resolves to "no lyrics",
  matching the C# `catch { return null; }`. Parsing then reuses `okp_core::lrc` unchanged.
- **Audio gate.** The Windows overlay shows only when `MediaFormats.IsAudio(path)` **and** mpv
  reports no video plane. The Linux surface uses the extension gate alone
  (`okp_core::media_formats::is_audio`): it is portable, deterministic, and already keeps the
  surface entirely off the video-first player, which is the guarantee that matters for this slice.
- **Deferred LRCLIB seam (not stubbed).** The metadata-keyed cache and the LRCLIB exact/fuzzy fetch
  are intentionally out of scope. On Windows the request is gated by the private-session flag
  (`AllowNetwork`/`AllowCacheWrite` both flip off in a private session; only the sidecar is consulted
  and no metadata leaves the machine). A later Linux port must wire that same policy through before
  adding `read_cached`/`fetch_lrclib` beside `read_sidecar`; until then the Linux experience is
  sidecar-first and fully local, so no private-session/history constraint is crossed.

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

## Subtitle track roles and format boundary → `okp_core::subtitle_tracks` / `subtitle_format` (Linux shell extraction)

- **No C# core counterpart; captures the Windows shell rule.** Windows has no `OkPlayer.Core`
  module for this — the classification lives in the `PlayerViewModel.ReadTracks` shell method.
  Its `isPrimary = selected && !isSecondary` guard (issue #195) exists because mpv reports
  `track-list/N/selected = yes` for BOTH the primary and the secondary caption, so the raw flag
  alone would draw a stray checkmark on the secondary track in the primary picker and make an
  "active secondary, no primary" file read as though a primary were selected. `subtitle_track_role`
  / `is_primary_subtitle` / `has_primary_subtitle` reproduce that rule for the Linux shell under
  the freeze-boundary; the secondary is matched by id against `secondary-sid` (mpv resolves it to
  a concrete id, never `auto`), and everything else mpv flags as selected — including an
  auto/default pick — is the primary.
- **`can_offer_secondary` mirrors `CanUseSecondarySubtitle`.** Windows gates the secondary picker
  with `subs.Count >= 2 || !secondaryOff`; the Rust `can_offer_secondary(count, secondary_active)`
  is the same predicate. Both shells offer the picker once a dual-subtitle choice exists or a
  secondary is already active (so an mpv-carried `secondary-sid` in a single-track file can always
  be switched back off), and hide it otherwise to keep a single-track flyout calm.
- **ASS/SSA preset applicability is currently Linux-only.** Windows already states in Settings
  that ASS/SSA keeps built-in styling, but it has no portable format classifier or per-track
  applicability state. `subtitle_format` extends its merged text/image classification with mpv
  codec metadata plus the external filename extension (needed because FFmpeg commonly reports SSA
  as `ass`) to distinguish supported text, native-style, image, and unknown preset states.
  `subtitle_tracks` projects that state from the selected primary track. The Linux picker then
  disables its preset cycle for native/image/unknown tracks while preserving the global preset for
  supported text tracks. This is an intentional compatibility note for issue #227, not a port of
  untested C# shell logic.
- **Media surface wording is Linux-only.** The Linux Media Info window names each subtitle slot
  `Primary` / `Secondary` in the track detail (`okp-mpv` reads `secondary-sid` alongside the
  track list). Windows has no equivalent media-info surface; the wording is presentation local to
  the GTK shell.

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
- **Update decisions are now shared state.** `UpdateOfferState` owns the portable
  available → skipped/installing → failed-or-installed transitions, exact-version
  prompt suppression, Install-anyway projection, and retry eligibility. GTK
  retains only the non-portable verified package/update-manager payload and
  renders the state in native surfaces. Windows has no equivalent Skip-version
  behavior today, so there is no C# test suite to port; a future Windows consumer
  can project the same state without moving decision logic into its shell.
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
- **Skipped update versions are per channel.** Linux writes optional
  `updates.skipped_versions.public` / `.candidate` exact-version strings. Older
  Linux documents and migrated Windows documents default both slots to absent;
  Windows currently ignores them because its updater has no Skip-version UX.
- **Linux-only vs Windows-only fields coexist.** The picture adjustments
  (`video.brightness`/`contrast`/`saturation`/`gamma`), `playback.auto_advance`, `repeat`, and
  `shuffle` are Linux-only; `playback.gapless` is a shared reserved preference that shells must
  capability-gate before applying. `subtitles` and `appearance` still carry fields one shell may
  not expose, while `privacy.history_retention_days` is now read and written by both desktop
  tracks. Each shell reads the subset it understands and carries the rest through untouched on
  save, so the shared schema grows without either side dropping the other's state. Empty optional
  sections remain omitted from a default Linux document.

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
- **Video geometry is a Linux-led canonical preference.** `preferences.video_geometry` stores
  the normalized aspect preset, linear zoom multiplier, pan offsets, quarter-turn rotation,
  fill-screen crop choice, and deinterlace toggle in the app index. The pure
  `okp_core::video_geometry` model owns bounds, menu eligibility, and action transitions; the GTK
  shell only renders it and maps the resulting values to libmpv. Linux writes this field only for
  local files and only outside private sessions, then restores it through the same pending
  preference path as tracks, delays, and speed. The current Windows `FileRecord` has no geometry
  fields and its context-menu adjustments remain session-only, so a Windows-dialect migration
  leaves `video_geometry` absent; this is the recorded parity gap until Windows adopts the
  canonical history document.
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
- **Retention and private-session mutation policy are now shared behavior.**
  `History::prune_older_than` ports the Windows `HistoryService.PruneOlderThan` cases: a zero or
  negative window keeps everything, records strictly older than the cutoff are removed, records
  inside the window remain, and an absent/unparseable timestamp (canonical `0`) is preserved.
  `HistoryWriteMode` gates progress, playback-preference, and bookmark creation in private mode;
  existing records remain readable and explicit deletions (`clear`, bookmark removal) remain
  available. The core tests cover those Windows privacy/retention cases, while the GTK store owns
  atomic JSON IO and rolls an in-memory clear/prune back if persistence fails.
- **Poster and titled-user-chapter authoring remain the documented Linux gap.** Windows also gates
  `SetPoster` and `AddUserChapter` in private mode. Linux does not persist poster paths or author
  titled `UserChapters`: it derives posters into a cache and currently exposes position bookmarks
  only. The GTK projection therefore keeps cached posters readable in private mode but schedules no
  new poster generation, and there is no Linux user-chapter writer to gate. The existing schema
  fields still round-trip untouched for future parity.

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

## Network URL and live-stream state (Linux, PRD §3)

Direct `http(s)://` playback plus polished loading, buffering, unknown-duration, and error
states are a Linux-shell concern for now (issue #208); the portable model they read from is
pure core so the Windows shell renders the same model once its port lands.

- **Load-state model (`okp_core::network_media`).** `MediaLoadState` (`Idle` / `Loading` /
  `Playing` / `Failed`) is the single source of truth the loading, buffering, and error
  surfaces read from. The shell only *transitions* it — on `load_url`/`load_file`
  (`Loading`), the engine's `FileLoaded` lifecycle event (`Playing`), and a reported failure
  (`EndFile::Error` or a load command returning `Err` → `Failed`). `classify_load_state`
  encodes the priority (a failure wins over a stale `FileLoaded` flag); the shell never owns
  that state machine.
- **Live / unknown-duration sentinel.** `network_media::format_duration_total(is_url,
  duration)` renders the padded clock when the duration is known and the `--:--` sentinel
  only for a URL whose duration has not resolved — the Live/URL unknown-duration readout,
  not the broken `00:00` total. `is_live_or_unknown_duration(is_url, duration)` gates the
  live classification: only a URL whose duration has not resolved reads as live, since a
  local file with no observed duration is just *loading* and renders `00:00`. The shell's
  seek range still clamps to 0 so the bar stays progress-only / disabled rather than running
  broken timeline math. (`time_code::format_duration` remains the ungated core helper that
  always uses the sentinel for an unknown duration.)
- **Failure presentation (`okp_core::network_media`).** `LoadFailureAction`
  (`Retry` / `OpenAnother` / `CopyDetails`, in that order via `LOAD_FAILURE_ACTIONS`) is the
  contract the shell's failure dialog renders. `failure_detail(url, reason)` builds the
  copyable summary — a short URL + reason line, never raw internal logs, so the primary UI
  never dumps engine trace into the clipboard. The Linux failure dialog is URL-only: a
  local-file error still transitions the surface (the overlay line) but does not arm the
  Retry/Open-another dialog (`last_load_url` is cleared, so a previous URL's stale value
  can't arm the dialog for a later local-file failure).
- **Stale `EndFile::Error`.** A queued `EndFile::Error` can outlive the source it belongs to
  (URL A fails, then the user starts URL B before the next poll drains the queue). The pump
  snapshots the ended source's path/URL in `MpvEvent::EndFile { path }` (read from mpv's
  `path` on the reader thread), and the shell's `apply_endfile_error` drops the error when
  that path no longer matches the current source, so A's stale error can't fail B or arm the
  dialog with A's reason. A missing path tag falls back to applying so a genuine failure is
  never under-reported.
