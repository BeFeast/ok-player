# Session protocol fixtures

Golden JSON documents for the `okp-session` protocol (`schema_version` 1).
`rust/crates/okp-session/tests/golden.rs` checks that every Rust type
serialises to exactly these documents and decodes them back; the macOS shell's
`Codable` tests are meant to decode the same files. `enums.json` lists every
value of every enum. Change a fixture only together with the protocol. Whether
a change needs a new `SCHEMA_VERSION` is decided by "Compatibility within a
version" in `docs/session-abi.md`: additions a shell can skip keep the version,
and anything else bumps it.

The playlist's documents are the `playlist-*.json` files, checked the same way
by `tests/golden_playlist.rs`; `playlist-enums.json` lists the playlist's
enums. They stay in this directory, not a subdirectory, so that a shell test
listing this directory notices them.
