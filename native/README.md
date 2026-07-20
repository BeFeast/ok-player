# native/ — bundled engine binaries (not committed)

OK Player links **libmpv** (the render API) and renders through **ANGLE** (OpenGL ES → D3D11).
These native binaries are **not** stored in git (they are large and have their own upstream
release cadence). They are fetched into this folder by `scripts/fetch-natives.ps1` and copied
to the build output by the `OkPlayer.Render` / `OkPlayer.App` projects.

```
native/
├─ libmpv/
│  ├─ libmpv-2.dll          # GPL build, x64  (the engine — ~115 MB)
│  ├─ libmpv.dll.a          # import lib (reference)
│  └─ include/mpv/*.h       # client.h, render.h, render_gl.h (P/Invoke reference)
├─ ffmpeg/
│  └─ ffmpeg.exe           # GPL win64 static build (~140 MB) — media processing (sync clips, cut/convert)
└─ angle/                   # libEGL.dll, libGLESv2.dll, d3dcompiler_47.dll (provenance TBD — see render notes)
```

## Provenance

- **libmpv** — `mpv-dev-x86_64-*.7z` from <https://github.com/zhongfly/mpv-winbuild> (or shinchiro).
  GPL build (full features), matching OK Player's GPL-3.0-or-later license.
- **ffmpeg** — `ffmpeg-*-win64-gpl.zip` (static) from <https://github.com/BtbN/FFmpeg-Builds>. GPL build,
  matching OK Player's license. Bundled by `OkPlayer.App` next to the exe; used by `FfmpegRunner` for media
  processing. Only `ffmpeg.exe` is taken (not ffprobe/ffplay) — media inspection goes through libmpv.
- **ANGLE** — see the render pipeline notes; ANGLE ships with the Windows App SDK runtime and/or
  is supplied by the GL binding layer.

When distributing, add `THIRD-PARTY-NOTICES.md` (mpv/GPL + ANGLE notices) at the repo root.

## Linux native presentation

The Debian and AppImage lanes link system GTK and platform graphics libraries,
but package the pinned patched libmpv and its non-platform media-runtime closure
beside the executable. Candidate builds verify that every dynamic object stays
inside that private directory or resolves to a package declared by the Debian
metadata; hosted release preparation adds a mandatory clean foreign-distro
container check. On Wayland, video renders into a desynchronized `wl_subsurface` with its own EGL
window and swap boundary. The subsurface sits below the transparent GTK shell, so titlebar, OSC,
panels, popovers, subtitles rendered by libmpv, and input remain unchanged while video presentation
bypasses `GtkGLArea` and GSK composition. The shell supplies GDK's `wl_display*` to libmpv as
`MPV_RENDER_PARAM_WL_DISPLAY`, retaining the GDK display until the render context is freed for
direct VAAPI interop. X11 uses the compatibility `GtkGLArea` path; a Wayland A/B run can explicitly
select it with `OKP_VIDEO_BACKEND=gtk`. Standard and Mini-player chrome must both keep the GTK
parent transparent; the live Wayland presentation harness includes a native Mini-player row so an
opaque compact parent cannot silently cover a still-presenting child surface.

The Fedora RPM makes this boundary enforceable: its spec requires
`pkgconfig(mpv)`/`mpv-libs`, sets `OKP_REQUIRE_SYSTEM_MPV=1`, and installs no
libmpv or FFmpeg shared library. Broader codecs may be supplied by repositories
the user explicitly chooses, but package scriptlets never enable one.
