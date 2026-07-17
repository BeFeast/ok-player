# Fedora native RPM and COPR beta lane

OK Player supports native RPM builds for Fedora 43 and Fedora 44. The package
links to Fedora's system `mpv-libs`; it does not bundle libmpv or enable any
third-party repository. Stock Fedora therefore keeps its intentionally limited
codec surface.

## Build an SRPM

The source builder creates normalized project and Cargo-vendor archives from
the locked workspace, then writes an SRPM:

```bash
./scripts/package-linux-rpm.sh --version 0.11.0-beta.1
```

The resulting SRPM builds without network access after mock installs the
declared Fedora build dependencies. The pull-request workflow rebuilds it in
clean Fedora 43 and 44 mock chroots, runs the workspace tests from `%check`,
runs `rpmlint`, then exercises install, upgrade, removal, desktop/AppStream
integration, and preservation of per-user JSON configuration.

To reproduce that full matrix locally on a host with Docker:

```bash
sudo -E ./scripts/test-linux-rpm.sh 43
sudo -E ./scripts/test-linux-rpm.sh 44
```

## COPR beta configuration

The repository contains `.copr/Makefile`, the COPR custom source method. A
public beta project should enable only the supported chroots:

- `fedora-43-x86_64`
- `fedora-44-x86_64`

Configure the COPR project to use the repository's custom source method and
publish it as a beta repository. COPR ownership, project name, and credentials
belong in COPR/GitHub settings, never in this repository. No package scriptlet
enables COPR or RPM Fusion on a user's machine.

## Codec behavior

`ffmpeg-free` and `mpv-libs` come from Fedora. If libmpv reports that no decoder
can be initialized, the Fedora build labels the problem **Codec unavailable**
and explains that optional RPM Fusion multimedia packages may add support. The
Copy details action links to RPM Fusion's public configuration page and states
that OK Player never enables third-party repositories automatically.

Renderer/GPU initialization failures and unreadable files/streams have separate
messages, so a stock Fedora codec limitation is not reported as a renderer or
application crash. Acceptance results for stock Fedora and RPM Fusion remain
separate in the Fedora acceptance manifest described in
[`fedora-acceptance.md`](fedora-acceptance.md).

## Live acceptance boundary

The mock workflow proves package construction, declared dependencies, metadata,
and package-manager lifecycle behavior. It does not prove GNOME/KDE Wayland
portals, drag/drop, compositor focus, or real hardware decode. Run the existing
Fedora acceptance harness separately in `stock-repos` and `rpmfusion` states;
report H.264/H.265 results on their distinct codec sources.
