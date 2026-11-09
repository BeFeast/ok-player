# Transport fixtures

Golden documents for the `transport` query and the `nudge_volume` and
`toggle_mute` commands (`docs/session-abi.md`, "Transport controls").
`tests/golden_transport.rs` checks each against the Rust types;
`query-reply-transport.json` is also the projection of the snapshot that test
builds, so it pins the core rules the projection applies. The macOS shell's `Codable` tests read the same files.
