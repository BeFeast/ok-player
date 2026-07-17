# Fedora release acceptance

Fedora validation is not just an RPM build. Stock Fedora exposes a restricted
codec surface, keeps SELinux **enforcing**, and ships different portal, Mesa, and
PipeWire versions across GNOME (Workstation) and KDE (Plasma) Wayland sessions.
This contract is a repeatable acceptance harness that runs inside a constrained
KVM guest, keeps policy enforcing, and exports a bounded, machine-readable
evidence bundle without embedding any lab topology in the repository.

The Debian/AppImage acceptance levels remain the general Linux contract
([`linux-release-acceptance.md`](linux-release-acceptance.md)); this document adds
the Fedora-specific gates.

The current native RPM matrix is Fedora **43** and **44** on x86_64. The core
validator rejects an older/newer release being presented as current acceptance.
RPM construction and COPR setup are documented in
[`fedora-rpm.md`](fedora-rpm.md).

## Test states

A run targets exactly one test state. The state selects which repositories and
package delivery are under test, and whether a package artifact hash is a hard
precondition:

| `--state`     | Manifest `test_state`  | Codec source        | Artifact required |
|---------------|------------------------|---------------------|-------------------|
| `stock-repos` | `stock-repos`          | stock Fedora only   | no                |
| `rpmfusion`   | `rpm-fusion-codecs`    | stock + RPM Fusion  | no                |
| `flatpak`     | `flatpak-package`      | (package bundle)    | **yes** (Flatpak) |
| `native-rpm`  | `native-rpm-copr`      | (package deps)      | **yes** (RPM)     |
| `copr`        | `native-rpm-copr`      | (package deps)      | **yes** (COPR)    |

Stock and RPM Fusion codec results are recorded on distinct `CodecSource`
values, so a codec-complete pass can never be confused with a stock pass. An
RPM Fusion codec source is rejected outright in the `stock-repos` state.
H.264 and H.265/HEVC are required in every codec report. A native RPM manifest
may describe either a stock or RPM Fusion run, but never both: the validator
rejects mixed sources so the two reports remain independently auditable.

## SELinux stays enforcing

The harness never relaxes SELinux. It reads the current mode with `getenforce`
and collects AVC denials for the current boot (`ausearch`, falling back to
`journalctl`), reducing each denial to its SELinux context fields
(`comm`/`scontext`/`tcontext`/`tclass`) so no host path leaks into the bundle.

The manifest **fails** if SELinux is permissive or disabled, or if any recorded
AVC denial has no justification. Known-benign denials can be justified with
`--avc-justify FILE` (a JSON object mapping signature → reason); a justified
denial passes, an unexplained one blocks release.

## Codec honesty

Stock-codec limitations are an expected diagnostic path, not a silent playback
success. Each codec check records one of:

- `decoded` — the stream played;
- `diagnosed-unsupported` — the codec is unavailable and the shell surfaced an
  honest diagnostic (the expected stock path for patent-encumbered codecs; the
  diagnostic text is required);
- `silent-failure` — playback reported success or hung producing no frames and
  no diagnostic. This always fails.

For the Fedora RPM, the missing-codec diagnostic names the system codec boundary
and gives optional RPM Fusion remediation without enabling or requiring it.
Renderer/GPU failures are classified separately and do not show codec advice.

## Media profiles

The **low-resource** profile (UI responsiveness plus 1080p H.264) is
capability-aware and must run even in a virtual-GPU guest. The **real-hardware**
profile (4K60, HDR, VA-API) is GPU-specific: in a virtual-GPU guest it is
explicitly `skipped` and must carry renderer capability evidence. The contract
rejects a real-hardware `pass` on a virtual GPU as a false pass, and rejects a
silent `not-run`.

The guest renderer and VA-API availability are detected from `glxinfo`/`eglinfo`
and `vainfo`; a software or paravirtual renderer (`llvmpipe`, `virgl`, …) marks
the guest as virtual.

## Coverage

Every run accounts for install, update, removal, desktop entry, MIME
associations, AppStream, file portals, drag/drop, screenshots, audio/PipeWire,
MPRIS, subtitles, stereo downmix, Settings sizing, menus, window dragging, and
window geometry. Each area carries a status. File portals, drag/drop, and window
dragging are live-desktop only and may remain `not-run` for operator QA; every
other area must run, and a failing area fails the run. A blocked non-operator
area (unmet precondition) blocks rather than fails.

## Running the harness

Non-interactively, after a clean VM snapshot restore:

```bash
./scripts/run-linux-fedora-acceptance.sh --state stock-repos --out out/fedora
```

For a packaged state, pass the artifact so its hash is bound into the manifest:

```bash
./scripts/run-linux-fedora-acceptance.sh --state flatpak \
  --artifact-file org.okplayer.OkPlayer.flatpak --out out/fedora
```

Live playback, media, and coverage steps produce JSON-array fragments that are
overlaid onto the auto-collected facts with `--codecs`, `--media`, and
`--coverage`. The collector shapes everything into
`fedora-acceptance-manifest.json` and validates it; all pass/fail/blocked
decisions live in `okp_core::fedora_acceptance`, not in the script.

The validator exit codes are distinct: `0` pass, `1` fail, `3` blocked
precondition. A missing artifact is a blocked precondition, never a false pass.

## Evidence bundle and beta reuse

The emitted manifest carries the exact package hash, Fedora version and desktop,
SELinux state, AVC denials, renderer/VA-API capability, codec state, and per-area
pass/fail evidence — enough to reconstruct the run without the guest. It embeds
no private hostnames, addresses, or credentials, and the same manifest can be
attached to a future public beta acceptance record.

The schema and every decision rule are unit-tested in
`rust/crates/okp-core/src/fedora_acceptance.rs`; those tests are the executable
spec for this contract.
