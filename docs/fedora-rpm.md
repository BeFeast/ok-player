# Fedora RPM and COPR beta lane

OK Player's native Fedora package targets the currently supported Fedora 43
and Fedora 44 x86_64 releases. It is a separate beta packaging lane from the
Ubuntu candidate builder: the RPM is built from the same source commit, but it
is not added to the Debian/AppImage updater feed.

## Package contract

The canonical spec is
[`rust/packaging/fedora/ok-player.spec`](../rust/packaging/fedora/ok-player.spec).
It installs:

- `/usr/bin/ok-player`;
- the canonical desktop entry, MIME declarations, icon set, and AppStream
  MetaInfo from the shared Linux packaging directory;
- GPLv3 license text and `THIRD-PARTY-NOTICES.md`.

The binary dynamically links Fedora's `mpv-libs`. The spec declares
`BuildRequires: pkgconfig(mpv)` and `Requires: mpv-libs`; the build exports
`OKP_REQUIRE_SYSTEM_MPV=1`, which rejects a source-tree libmpv path. No mpv or
FFmpeg shared library is copied into the RPM.

Rust crates are captured as a deterministic vendor source archive for the
SRPM's offline `%build` and `%check` phases. This is source material, not a
second native media engine. The source archive, vendor archive, commit marker,
and SRPM use the commit timestamp as `SOURCE_DATE_EPOCH`, sorted entries,
numeric ownership, and stable gzip/zstd settings.

Build the source package from a clean committed tree:

```bash
./scripts/package-linux-rpm-source.sh
```

Inside a clean Fedora root with the normal build tooling installed, run the
complete build/lint/transaction gate:

```bash
FEDORA_VERSION=44 ./scripts/run-linux-rpm-checks.sh
```

That command first produces the complete source set twice and requires every
source archive, checksum manifest, and SRPM to be byte-identical. It then
installs BuildRequires from the SRPM, rebuilds a lower-release candidate and
the current RPM, runs `rpmlint`, and uses `dnf` to exercise clean install,
upgrade, removal, declared dependency resolution, dynamic system libmpv
linkage, and preservation of per-user JSON configuration under
`XDG_CONFIG_HOME`. `rpmlint` errors fail the beta gate. Warnings are retained in
`rpmlint.txt` and must be accounted for in the PR/release evidence rather than
hidden.

The `Fedora RPM` GitHub workflow repeats this in clean Fedora 43 and 44
container roots. COPR remains the authoritative clean `mock` chroot surface.

## COPR project setup

The repository includes `.copr/Makefile` for COPR's custom source method. A
public beta project should use:

- project name such as `ok-player-beta`;
- Fedora 43 x86_64 and Fedora 44 x86_64 chroots;
- the public repository clone URL and the intended branch/tag;
- custom source (`make srpm`) so `.copr/Makefile` emits the SRPM;
- source-builder packages `cargo`, `git`, `rpm-build`, `tar`, `gzip`, and
  `zstd`.

Do not store COPR API tokens, private hostnames, builder addresses, or account
specific project IDs in this repository. Enabling the resulting beta COPR is
an explicit user/operator action; the RPM has no repository-enabling scriptlet.

## Stock Fedora and RPM Fusion codec runs

Fedora's stock codec surface and an explicitly configured RPM Fusion surface
are two separate acceptance runs. Both must test H.264 and H.265/HEVC and write
separate manifests:

```bash
./scripts/run-linux-fedora-acceptance.sh \
  --state native-rpm \
  --artifact-file ok-player.rpm \
  --codecs stock-codecs.json \
  --media media.json \
  --coverage coverage.json \
  --out out/fedora-stock

./scripts/run-linux-fedora-acceptance.sh \
  --state native-rpm \
  --artifact-file ok-player.rpm \
  --codecs rpmfusion-codecs.json \
  --media media.json \
  --coverage coverage.json \
  --out out/fedora-rpmfusion
```

Every codec entry in one manifest must use the same `source` value. The core
validator rejects mixed stock/RPM Fusion evidence and rejects a manifest that
omits either H.264 or HEVC.

On stock Fedora, an unavailable codec must produce the **Codec unavailable**
diagnostic. The RPM build explains that OK Player uses the system codec set and
offers RPM Fusion as an optional user choice while stating that the application
will not enable third-party repositories. Renderer/GPU initialization failures
use a different diagnostic and never suggest codec installation.

A headless or container transaction test proves package layout and dependency
resolution only. Portal, chooser, drag/drop, compositor, clipboard, focus, GPU,
and live codec playback evidence still comes from the Fedora VM acceptance run.
