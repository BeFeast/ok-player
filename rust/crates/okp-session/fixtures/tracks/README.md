# Track fixtures

Golden documents for the `tracks` query and the `add_subtitle`,
`set_subtitle_scale` and `nudge_subtitle_scale` commands (`docs/session-abi.md`,
"Tracks and subtitles"). `tests/golden_tracks.rs` checks each against the Rust
types. The labels in `query-reply-tracks.json` are the core's
(`okp_core::track_label`) for the tracks that test describes. The macOS shell's
`Codable` tests read the same files.
