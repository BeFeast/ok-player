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
