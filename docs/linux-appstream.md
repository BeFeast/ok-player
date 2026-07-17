# Linux AppStream MetaInfo

The Linux packages ship an AppStream MetaInfo document so OK Player presents
correctly in software centres (GNOME Software, KDE Discover) and so the same
metadata can seed a future Flatpak/RPM manifest. Valid MetaInfo is also a
Flathub submission requirement.

## Canonical source

- **File:** [`rust/packaging/linux/com.befeast.okplayer.metainfo.xml`](../rust/packaging/linux/com.befeast.okplayer.metainfo.xml)
- **Component id:** `com.befeast.okplayer` (reverse-DNS, matches the desktop
  entry `com.befeast.okplayer.desktop` and the icon `com.befeast.okplayer`)
- **Component type:** `desktop-application`
- **Project license:** `GPL-3.0-or-later` (matches [`LICENSE`](../LICENSE))
- **Metadata license:** `CC0-1.0`
- **Homepage / bugtracker / VCS:** `https://github.com/BeFeast/ok-player`

This file is packaging-neutral: it is the single source both the Debian and
Velopack/AppImage lanes install, and the intended source for Flatpak/RPM.

## Install path

Both package lanes install the MetaInfo to the standard location so any
consumer (software centre, `appstreamcli`, Flatpak builder) finds it:

```
/usr/share/metainfo/com.befeast.okplayer.metainfo.xml
```

- Debian: [`scripts/package-linux-deb.sh`](../scripts/package-linux-deb.sh)
- Velopack/AppImage: [`scripts/package-linux-velopack.sh`](../scripts/package-linux-velopack.sh)

## Validation and CI gates

`appstreamcli validate --pedantic` is the authoritative validator. Two gates
run it:

- **Every pull request** (`.github/workflows/rust.yml`) runs
  [`scripts/smoke-linux-appstream.sh`](../scripts/smoke-linux-appstream.sh),
  which validates the source MetaInfo, validates the assembled installed file
  tree, composes catalog metadata (which fails when the launchable desktop id or
  the icon is missing/mismatched), and asserts the id, launchable, license, and
  homepage fields. This is what fails CI on invalid or incomplete metadata.
- **The Linux release build** (`.github/workflows/release-linux.yml`) runs
  [`scripts/smoke-linux-appstream-deb.sh`](../scripts/smoke-linux-appstream-deb.sh)
  against the exact built `.deb`: it installs the package, confirms the MetaInfo
  lands and validates, is discoverable by name and desktop id in a local
  AppStream query, and is removed again on uninstall.

Run the source gate locally (needs the `appstream` and `appstream-compose`
packages):

```sh
./scripts/smoke-linux-appstream.sh
```

## Screenshots — pending release gate

Flathub requires at least one screenshot, and the MetaInfo intentionally does
**not** yet declare a `<screenshots>` element: the final approved Linux captures
and their hosting URLs are not settled. Rather than invent a URL, this is left
as an explicit, documented release gate.

Before a Flathub submission (or any store listing that requires screenshots):

1. Publish the approved Linux capture(s) at a stable URL (for example under the
   repository's release assets or the Pages site).
2. Add a `<screenshots>` block to the MetaInfo referencing those URLs, marking
   one `type="default"`.
3. Re-run `./scripts/smoke-linux-appstream.sh`; `appstreamcli validate
   --pedantic` will then also check the screenshot entries.

The absence of screenshots does not fail `appstreamcli validate` (they are
recommended, not required by the spec), so CI stays green; this section is the
record that the asset is deliberately outstanding.
