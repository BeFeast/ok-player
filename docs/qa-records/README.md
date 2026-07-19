# QA acceptance records

`docs/qa-records/` is the canonical in-repository location for durable QA,
acceptance, and traceability outcomes. Greptile reviews this directory; it is
not an artifact dump or a substitute for full logs.

## File contract

Every QA-only or acceptance-only pull request must add one Markdown record
named `YYYY-MM-DD-issue-NNN.md`, using the UTC date and issue number. Start from
[`TEMPLATE.md`](TEMPLATE.md). The record is the required reviewable file even
when the issue explicitly forbids source, packaging, feed, or UI changes.
Empty commits and pull requests with no file changes are invalid because the
review gate cannot inspect or approve them.

Each record must contain:

- the issue, decision, and exact scope of the run;
- the repository base SHA, tested source SHA, candidate source SHA when it is
  distinct, package identity, and related workflow or pull request identifiers;
- a sanitized environment description: operating system, desktop/session,
  package lane, relevant hardware class, and important runtime versions;
- a result matrix with the expected result, actual result, evidence level, and
  `PASS`, `FAIL`, `BLOCKED`, or `NOT RUN` for every required row;
- SHA-256 checksums for every package, manifest, screenshot, evidence bundle,
  or other artifact used to support the decision;
- durable links to complete logs and externally stored artifacts; and
- explicit limitations, remaining holds, and operator-only checks that were
  not proven by the recorded environment.

Use repository-relative names and public URLs. Never include local machine
paths, hostnames, credentials, private addresses, or infrastructure details.
Large logs, binaries, packages, screenshots, and generated evidence bundles
remain outside git; the Markdown record links to them and binds them by digest.

## Pull request flow

1. Run the issue's exact acceptance contract against one identified source and
   artifact set.
2. Store full logs and generated artifacts in the issue-approved durable
   location and calculate SHA-256 checksums.
3. Add `docs/qa-records/YYYY-MM-DD-issue-NNN.md` with the complete matrix and
   provenance. Do not use an empty traceability commit.
4. Run the repository gates required by the issue and verify the record contains
   no private environment details.
5. Open the pull request normally so Greptile can review the record. Link the
   issue and the full logs from the pull request body as well.

If a required reference, artifact identity, log, or live-desktop state is
missing, record the row as `BLOCKED` or `NOT RUN`; do not infer a pass.
