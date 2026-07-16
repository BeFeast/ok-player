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

## Linux scaled-workarea fixture

`scripts/generate-linux-acceptance-media.sh` deterministically generates
`fit-4k.mkv`, a 3840×2160 HEVC Main10 (`yuv420p10le`) solid-frame clip used by
the Linux main-window fit smoke and real GNOME/Wayland acceptance. The generated
manifest records and validates the codec and pixel format so a local ffmpeg
default cannot silently downgrade the acceptance source.

The headless smoke repeats the clip on a 1024×768 workarea and on a simulated
3840×2160 display at scale factor 2. Real GNOME/Wayland acceptance must still
repeat those states on the target 4K display because Xvfb does not prove
compositor placement, desktop panel reservations, or fractional-scale policy.
