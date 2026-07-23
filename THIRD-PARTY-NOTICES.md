# Third-Party Notices

OK Player is distributed under the **GNU General Public License, version 3 or later**
(see [`LICENSE`](LICENSE)). It links and bundles third-party components. Their notices and
licenses are reproduced or referenced below. Because OK Player links **libmpv** built with its
full (GPL) feature set, the combined work is distributed under the GPL — this is intentional and
mirrors mpv.net and IINA.

## libmpv (mpv)

- **Project:** mpv — <https://mpv.io> · <https://github.com/mpv-player/mpv>
- **Components bundled:** `libmpv-2.dll` in Windows packages and a source-built
  `libmpv.so` in the Flatpak beta package.
- **License:** GPL-2.0-or-later when built with GPL components (as bundled here); core mpv is
  otherwise LGPL-2.1-or-later. Full text: <https://github.com/mpv-player/mpv/blob/master/LICENSE.GPL>
- **Copyright:** © the mpv developers and the MPlayer/mplayer2 projects it descends from.

The native Fedora RPM does **not** bundle libmpv or FFmpeg libraries. It
dynamically links Fedora's `mpv-libs` package and therefore uses the codec set
provided by the repositories the user has explicitly enabled. The Windows
distribution details below describe the bundled Windows runtime only.

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

## Obtaining the source

OK Player's source is at <https://github.com/BeFeast/ok-player>. The corresponding source for the
bundled GPL components (mpv, FFmpeg) is available from the upstream projects linked above; mpv's
Windows builds and their build scripts are published at <https://github.com/zhongfly/mpv-winbuild>
and <https://sourceforge.net/projects/mpv-player-windows/>, and the standalone `ffmpeg.exe` build
and its build scripts at <https://github.com/BtbN/FFmpeg-Builds>.
