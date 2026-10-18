# Third-Party Notices

OK Player is distributed under the **GNU General Public License, version 3 or later**
(see [`LICENSE`](LICENSE)). It links and bundles third-party components. Their notices and
licenses are reproduced or referenced below. Because OK Player links **libmpv** built with its
full (GPL) feature set, the combined work is distributed under the GPL — this is intentional and
mirrors mpv.net and IINA.

## libmpv (mpv)

- **Project:** mpv — <https://mpv.io> · <https://github.com/mpv-player/mpv>
- **Components bundled:** `libmpv-2.dll` in Windows packages, a patched
  `libmpv.so.2` in the Linux `.deb` and AppImage packages, and a source-built
  `libmpv.so` in the Flatpak beta package.
- **License:** GPL-2.0-or-later when built with GPL components (as bundled here); core mpv is
  otherwise LGPL-2.1-or-later. Full text: <https://github.com/mpv-player/mpv/blob/master/LICENSE.GPL>
- **Copyright:** © the mpv developers and the MPlayer/mplayer2 projects it descends from.

The native Fedora RPM does **not** bundle libmpv or FFmpeg libraries. It
dynamically links Fedora's `mpv-libs` package and therefore uses the codec set
provided by the repositories the user has explicitly enabled. Every other lane
— Windows, the Linux `.deb`, the AppImage and the Flatpak — ships libmpv and
the FFmpeg libraries inside the artifact.

mpv in turn incorporates or links, among others, FFmpeg, libass, libplacebo,
and zlib. The dominant licenses of the bundled build are listed below; the
authoritative, per-file licensing is in each upstream project. Flatpak installs
the upstream mpv copyright and GPL/LGPL license texts in the application license
directory.

## FFmpeg

- **Project:** FFmpeg — <https://ffmpeg.org> · <https://github.com/FFmpeg/FFmpeg>
- **Components bundled:** (1) the FFmpeg libraries inside `libmpv-2.dll`, and (2) a standalone
  `ffmpeg.exe` — a GPL `win64-gpl` build from <https://github.com/BtbN/FFmpeg-Builds> — used by OK
  Player for media processing (subtitle-sync audio clips; cut/convert/remux).
- **License:** GPL-2.0-or-later for these builds (FFmpeg is LGPL-2.1-or-later by default, but
  GPL-only components are enabled in the builds bundled here).
  Full text: <https://www.ffmpeg.org/legal.html>
- **Copyright:** © the FFmpeg developers.

The Flatpak beta links libmpv against the GNOME/Freedesktop runtime FFmpeg
libraries. The optional `org.freedesktop.Platform.codecs-extra` extension can
replace those runtime libraries with the expanded codec build; the extension is
distributed and licensed independently by Freedesktop/Flathub.

## libass

- **Project:** libass — <https://github.com/libass/libass>
- **Component bundled:** the source-built subtitle renderer in the Flatpak beta.
- **License:** ISC. **Copyright:** © the libass developers.

## libplacebo

- **Project:** libplacebo — <https://github.com/haasn/libplacebo>
- **Component bundled:** the source-built rendering library in the Flatpak beta.
- **License:** LGPL-2.1-or-later. **Copyright:** © libplacebo contributors.

## zlib

- **Project:** zlib — <https://zlib.net>
- **License:** zlib license. **Copyright:** © Jean-loup Gailly and Mark Adler.

## .NET runtime, Windows App SDK / WinUI 3, CommunityToolkit.Mvvm

- **.NET runtime & libraries** — © .NET Foundation and contributors, MIT License
  (<https://github.com/dotnet/runtime>). Bundled in self-contained builds.
- **Windows App SDK / WinUI 3** — © Microsoft Corporation, MIT License
  (<https://github.com/microsoft/WindowsAppSDK>). Bundled in self-contained builds.
- **CommunityToolkit.Mvvm** — © .NET Foundation and contributors, MIT License
  (<https://github.com/CommunityToolkit/dotnet>).

## Chrome icon set (Lucide, plus one first-party icon)

- **Component:** the symbolic icons the Linux shell resolves for its own chrome
  (`rust/crates/okp-linux-gtk/icons/okp-*-symbolic.svg`), compiled into the
  binary as a `GResource`. The set has two origins under two licences.
- **Lucide artwork** — every icon in the set except the one named below. The
  files are renamed into an `okp-` namespace so a user's own icon theme cannot
  shadow them (see issue #731), and each 2px stroke is rendered as a filled
  outline so GTK's symbolic recolour reaches it. The transport glyphs (play,
  pause, fast-forward, skip back, skip forward), the drag handle and the
  overflow dots are rendered as solid silhouettes of the same outlines, which
  is how they read at control sizes. The shapes are upstream's.
  - **Upstream:** <https://lucide.dev> — <https://github.com/lucide-icons/lucide>
  - **Version:** Lucide 0.544.0 (`lucide-static@0.544.0`), taken from the
    design hand-off's Lucide sprite and from upstream's SVGs for the icons that
    sprite does not carry.
  - **Copyright:** © 2022 Lucide Contributors. Portions © 2013–2022 Cole Bemis,
    from Feather — the attribution Lucide's own licence file makes.
  - **License:** ISC. Lucide's licence file is the ISC licence with a copyright
    line that reserves the portions taken from Feather to Cole Bemis under
    MIT; upstream does not mark per file which icons those are, so both texts
    are reproduced here and every icon file names both. The ISC text:

    > Permission to use, copy, modify, and/or distribute this software for any purpose
    > with or without fee is hereby granted, provided that the above copyright notice
    > and this permission notice appear in all copies.
    > 
    > THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
    > REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
    > FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
    > INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
    > OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
    > TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
    > THIS SOFTWARE.

    The MIT text, for the Feather-derived icons:

    > MIT License
    > 
    > Copyright (c) 2013-2022 Cole Bemis
    > 
    > Permission is hereby granted, free of charge, to any person obtaining a copy of
    > this software and associated documentation files (the "Software"), to deal in
    > the Software without restriction, including without limitation the rights to
    > use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
    > the Software, and to permit persons to whom the Software is furnished to do so,
    > subject to the following conditions:
    > 
    > The above copyright notice and this permission notice shall be included in all
    > copies or substantial portions of the Software.
    > 
    > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    > IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
    > FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
    > COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
    > IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
    > CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

- **`okp-process-working-symbolic.svg` (first-party)** — the spinner predates
  the Lucide set and was left as it was: original work drawn for OK Player by
  [`scripts/generate-chrome-icons.py`](scripts/generate-chrome-icons.py).
  - **License:** GPL-3.0-or-later, the same terms as the rest of OK Player
    (see [`LICENSE`](LICENSE)).
- **Supersedes:** earlier revisions of this section described an Adwaita set
  taken under the LGPL-3 option, and before that a wholly first-party
  script-drawn set. Neither describes what ships now.
  [`LICENSE.LGPL-3.0`](LICENSE.LGPL-3.0) continues to accompany every package:
  no component in the current set is under it, but every packaging lane and
  its gate is wired to stage it, so it stays until the lanes change together.
- **Not affected:** the application icon and desktop integration
  (`rust/packaging/linux/`), which are first-party.

## Obtaining the source

OK Player's source is at <https://github.com/BeFeast/ok-player>. The corresponding source for the
bundled GPL components (mpv, FFmpeg) is available from the upstream projects linked above; mpv's
Windows builds and their build scripts are published at <https://github.com/zhongfly/mpv-winbuild>
and <https://sourceforge.net/projects/mpv-player-windows/>, and the standalone `ffmpeg.exe` build
and its build scripts at <https://github.com/BtbN/FFmpeg-Builds>.
