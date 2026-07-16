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

# Stereo-downmix fixtures

Tiny (~1 KB) silent FLAC-in-Matroska clips used by the `okp-mpv` `stereo_downmix_*` tests to prove,
on the real engine, that the forced stereo downmix (`audio-channels=stereo`) reconfigures the live
audio chain to two output channels, that disabling restores the native channel count, and that
stereo/mono sources stay valid with the toggle in either state. Each is 2 s of `anullsrc` at 48 kHz.

| File | Channel layout |
|------|----------------|
| `downmix-5_1.mka` | 5.1 (6 ch) |
| `downmix-7_1.mka` | 7.1 (8 ch) |
| `downmix-stereo.mka` | stereo (2 ch) |
| `downmix-mono.mka` | mono (1 ch) |

Regenerate with ffmpeg:

```sh
for spec in "5.1:downmix-5_1.mka" "7.1:downmix-7_1.mka" "stereo:downmix-stereo.mka" "mono:downmix-mono.mka"; do
  layout="${spec%%:*}" out="${spec##*:}"
  ffmpeg -f lavfi -i "anullsrc=channel_layout=${layout}:sample_rate=48000" \
    -t 2 -c:a flac -metadata title="OK Player downmix fixture ${layout}" "$out"
done
```

Silence keeps the files tiny and deterministic; the tests only assert channel-count topology on
`audio-out-params`, never audible content.
