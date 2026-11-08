# Session protocol fixtures

Golden JSON documents for the `okp-session` protocol (`schema_version` 1).
`rust/crates/okp-session/tests/golden.rs` checks that every Rust type
serialises to exactly these documents and decodes them back; the macOS shell's
`Codable` tests are meant to decode the same files. `enums.json` lists every
value of every enum. Change a fixture only together with the protocol, and bump
`SCHEMA_VERSION` when an older shell could no longer read the result.

The playlist's documents live in `playlist/`, checked the same way by
`tests/golden_playlist.rs`; `playlist/enums.json` lists the playlist's enums.
