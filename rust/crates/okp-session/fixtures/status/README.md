# Status fixtures

Golden documents for the `status_surface` query and the `notice` event
(`docs/session-abi.md`, "Status surface and notices").
`tests/golden_status.rs` checks each against the Rust types. The reply fixtures
are the projections of the states that test builds, so `query-reply-failed.json`
also pins the core's classification, wording and actions for a failed load.
`enums.json` lists every value of the failure card's enums. The macOS shell's
`Codable` tests read the same files.
