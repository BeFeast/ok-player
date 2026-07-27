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
  shrink, and a line goes only when its test has stopped reading source text
  altogether - not when it merely acquired one behavioural assertion.
  Read the header of that file before you rely on the check: it lists what the
  detector does not see. In particular, **one assertion containing any call
  clears a whole test** - `assert_eq!(Some(1).unwrap(), 1);` beside three
  `source.contains` greps turns a rejection into a pass, measured. That gap is
  deliberate (closing it misclassifies real behavioural tests) and it is why
  this rule is a floor, not a verdict: a green check is not evidence that your
  test asserts anything, and adding a junk assertion to get past it is
  laundering, not a fix.
- **Nothing in the pull request declares it unfinished.** A `WIP` or `Draft`
  title prefix, the maestro WIP marker comment in the body, or an unresolved
  item under an acceptance heading (`Operator acceptance`, `Acceptance
  criteria`, `Live acceptance hold`, `Before merge`) blocks the merge until it
  is resolved. An unticked box counts wherever it sits in the section: putting a
  bold label or a rule above it does not close the block.
  Resolving means doing the work: never remove a marker or tick a box to turn a
  check green, and never delete an acceptance block that still applies.
- **An acceptance block contains checkboxes and nothing else.** A plain bullet
  or a sentence cannot record that anyone performed it, which is exactly how the
  historical acceptance holds reached `main` unperformed. Inside an acceptance
  section CI rejects every line that is not a checkbox or a nested heading -
  prose, plain bullets, `---`, bold labels, HTML tags - whether or not other
  boxes in the section are already ticked. Put context above the block.
  A section opened by a markdown heading runs to the next heading of its own
  level or above; one opened by a bold or bare label runs to the next **bold**
  label or heading. Neither ends at a rule, an HTML tag, or a bare label ending
  in a colon - a bare label is indistinguishable from a short prose hold that
  ends in one, and treating it as a terminator hides the bullets under it.
  Prefer a `##` heading to open the block: it is the only opener whose bounds
  are unambiguous. So anything appended below a
  trailing acceptance section (a review bot's summary, for instance) is read as
  part of it and will block the merge until it is moved above the block or given
  a heading of its own.
  An acceptance heading may be `## ATX`, a bold label, a bare label ending in a
  colon, or a setext heading (the phrase underlined with `---` or `===`).
  What CI cannot see: a hold written *outside* an acceptance heading, an
  acceptance heading phrased outside the recognised set, a hold wrapped in a
  balanced code fence or an HTML comment (both are blanked before any rule runs,
  so that a body may quote these rules), and a box that was ticked without the
  work being done. Nor does it read the body for meaning: only a body that is
  empty once HTML comments are removed is rejected, so the template with every
  placeholder comment left untouched passes. It over-blocks in one known shape: a results section such as
  `## Acceptance test results` is read as a hold, because the phrase match is
  deliberately loose. Rename the heading or move the prose above the block.
- **An operator acceptance block is honoured by the operator, not by a worker.**
  If an issue or a pull request says a packaged build must be verified by hand
  before merge, that verification happens before merge.
- **No unfinished-code markers reach `main`.** `todo!()`, `unimplemented!()`,
  and bare `TODO` / `FIXME` comments fail CI, in shipped source and in shipped
  configuration (workflows, manifests). A marker that names a tracking issue -
  `TODO(#1234)` - is accepted, because the work is then tracked rather than
  lost.
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
