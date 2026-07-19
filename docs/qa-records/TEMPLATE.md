# QA record: issue #NNN

- Date (UTC): `YYYY-MM-DD`
- Issue: `#NNN`
- Decision: `PASS | FAIL | BLOCKED`
- Scope: Describe exactly what this run accepts or rejects.

## Provenance

| Item | Exact identity |
| --- | --- |
| Repository base | `<40-character SHA>` |
| Tested source | `<40-character SHA>` |
| Candidate source | `<40-character SHA, or N/A with reason>` |
| Candidate/build | `<version, workflow run, and package identity>` |
| Related pull request | `#NNN` |

## Environment

| Item | Value |
| --- | --- |
| Operating system | `<distribution and version>` |
| Desktop/session | `<desktop, compositor, X11/Wayland/headless>` |
| Package lane | `<deb, AppImage, RPM, unpackaged, other>` |
| Hardware class | `<relevant CPU/GPU/display class; no hostname>` |
| Runtime versions | `<relevant toolkit, mpv, driver, or portal versions>` |

## Result matrix

| Check | Evidence level | Expected | Actual | Status | Full log |
| --- | --- | --- | --- | --- | --- |
| `<required row>` | `<model, headless, installed, live desktop>` | `<contract>` | `<observation>` | `PASS | FAIL | BLOCKED | NOT RUN` | `<durable URL>` |

## Artifact checksums

| Artifact | SHA-256 | Durable location |
| --- | --- | --- |
| `<logical artifact name>` | `<64 lowercase hexadecimal characters>` | `<durable URL>` |

## Limitations and holds

- List anything this environment cannot prove.
- List every failed, blocked, or not-run acceptance row and the resulting hold.
