# History protocol fixtures

Golden JSON documents for the `history` command, query and event groups of the
`okp-session` protocol (`schema_version` 1). `tests/history_golden.rs` checks
that every Rust value serialises to exactly these documents and decodes them
back; `enums.json` lists every value of every history enum. Change a fixture
only together with the protocol.
