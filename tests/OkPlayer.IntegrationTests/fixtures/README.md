# Subtitle render-test fixtures

Tiny synthetic clips used by `SubtitleOscClearanceTests` to prove, on rendered pixels, that the OSC-lift
raises captions clear of the controls for **every** subtitle kind (PRD P1-D9). Each is a 1280×720, ~3 s dark
H.264 clip with one embedded subtitle reading `SUBTITLE POSITION TEST`, shown 0–30 s.

| File | Subtitle track | Why it exists |
|------|----------------|---------------|
| `subtest.mkv` | embedded **ASS** (`subtest.ass`) | libass ignores `sub-margin-y`; this is the kind the old lift silently failed on |
| `subtest_text.mkv` | embedded **SRT/text** (`subtest.srt`) | text subs *do* honour `sub-margin-y` — the control case |

The `.ass` / `.srt` sources are kept alongside so the clips can be regenerated. They were built with ffmpeg:

```sh
# dark 1280x720, 3s test source
ffmpeg -f lavfi -i color=c=0x101010:s=1280x720:d=3 -c:v libx264 -pix_fmt yuv420p -t 3 base.mkv

# ASS variant
ffmpeg -i base.mkv -i subtest.ass -map 0:v:0 -map 1:0 -c:v copy -c:s ass -t 3 subtest.mkv

# text/SRT variant
ffmpeg -i base.mkv -i subtest.srt -map 0:v:0 -map 1:0 -c:v copy -c:s srt -t 3 subtest_text.mkv
```

White text on a near-black frame so the test can separate caption pixels from background by a simple luma cut.

## Temporary fixture lifetime

Rust filesystem tests use the shared `okp_test_fixtures::unique_temp_dir` guard. Keep that guard in
scope until the final filesystem access; derived paths do not retain the fixture. Tests that start a
detached NFO read must wait for its existing completion signal before explicitly closing the guard.

The guard removes fixtures after normal return and panic unwind. Destructors do not run after abort,
`process::exit`, `SIGKILL`, or OOM termination, so the external worker lease/runtime cleanup remains
responsible for those cases.

## Live Linux 4K60 fixture

The issue #312 acceptance clip is intentionally not committed. The operator supplies the same
private media file to standalone mpv and OK Player. The Wayland presentation harness rejects the
file unless `ffprobe` reports exactly one selected video stream with `codec_name=hevc`,
`profile=Main 10`, `pix_fmt=yuv420p10le`, `width=3840`, `height=2160`, and
`avg_frame_rate=60/1`. Source metadata alone is not success evidence; the harness measures the
final native EGL swap boundary for a continuous 15-second window.
