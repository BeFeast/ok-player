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
