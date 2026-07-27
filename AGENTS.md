# Repository worker guidance

## Definition of done

A pull request is done only when every line below is true of it. Anything less
is unfinished work, and unfinished work does not merge - no matter who or what
is doing the merging.

- **A regression test fails without the production change.** For a fix, revert
  the production change, watch the new test fail, restore it, watch it pass, and
  say so in the pull request body. A green suite is not evidence: it is equally
  green when the change does nothing.
- **Tests assert behaviour, not source text.** A test that loads a source file
  with `include_str!` and asserts that it `contains` a snippet passes against a
  completely broken implementation. CI rejects new ones. The allowlist of the
  ones that already exist, `.github/source-grep-test-allowlist.txt`, may only
  shrink.
- **Nothing in the pull request declares it unfinished.** A `WIP` or `Draft`
  title prefix, the maestro WIP marker comment in the body, or an unresolved
  item in an `Operator acceptance` block blocks the merge until it is resolved.
  Resolving means doing the work: never remove a marker or tick a box to turn a
  check green, and never delete an acceptance block that still applies.
- **An acceptance block contains checkboxes and nothing else.** A plain bullet
  or a sentence cannot record that anyone performed it, which is exactly how the
  historical acceptance holds reached `main` unperformed. CI rejects any prose
  or plain bullet inside an `Operator acceptance` block, including prose sitting
  next to boxes that are already ticked. Put context above the block.
- **An operator acceptance block is honoured by the operator, not by a worker.**
  If an issue or a pull request says a packaged build must be verified by hand
  before merge, that verification happens before merge.
- **No unfinished-code markers reach `main`.** `todo!()`, `unimplemented!()`,
  and bare `TODO` / `FIXME` comments fail CI. A marker that names a tracking
  issue - `TODO(#1234)` - is accepted, because the work is then tracked rather
  than lost.
- **The body states what the change does, how it was verified, and what it
  deliberately does not do.**

`scripts/check-unfinished-work.py` and `scripts/check-source-grep-tests.py`
enforce the mechanical parts of this, from the required `Rust workspace (Linux)`
job and from the `Merge gate` workflow. They are a floor, not the bar: passing
them does not make an unfinished change finished.

## QA and acceptance records

- A QA-only, acceptance-only, or traceability issue must add a real reviewable
  record at `docs/qa-records/YYYY-MM-DD-issue-NNN.md`.
- Do not open an empty pull request or use an empty commit as the issue's only
  durable result. If the issue forbids source changes, the QA record is the
  required repository change.
- Follow `docs/qa-records/README.md`. Record the result matrix, sanitized
  environment, exact source and candidate SHAs, artifact SHA-256 checksums,
  and links to complete logs.
- Keep large logs, screenshots, packages, and other generated artifacts out of
  the repository. Link to durable storage and bind each artifact by checksum.
- Never put machine paths, hostnames, credentials, or private infrastructure
  details in a QA record.
