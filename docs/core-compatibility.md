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
