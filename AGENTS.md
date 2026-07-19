# Repository worker guidance

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
