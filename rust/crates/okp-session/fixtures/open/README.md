# Open fixtures

Golden documents for the `open_formats` and `open_paths` queries
(`docs/session-abi.md`, "Opening files"). `tests/golden_open.rs` checks each
against the Rust types. Each reply is what the core rules answer for its query:
`query-reply-open-formats.json` lists okp-core's `media_formats` types, and the
three `query-reply-open-paths-*.json` documents are the plans for the paths of
`query-open-paths.json` (a folder the shell marks as one, a film, its subtitle,
an M3U file and a document) while media plays, for its subtitle alone while
media plays, and for that subtitle with nothing loaded. The macOS shell's `Codable`
tests read the same files.
