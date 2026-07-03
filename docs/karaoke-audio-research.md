# Karaoke audio pipeline — research note

**Scope:** research only (issue #111, post-1.0 far ideation from the tester interview).
This document evaluates options for a karaoke-oriented audio pipeline — real-time
vocal removal/isolation, pitch shift, and reverb/echo — and recommends a direction.
It commits to **no product work**; anything actionable here needs its own issue and,
where noted, a PRD amendment first.

---

## 1. What "karaoke" needs

From the issue, three DSP capabilities:

| Capability | Karaoke meaning | Hard part |
|---|---|---|
| Vocal removal / isolation | Mute the lead vocal so the user can sing | True isolation requires source separation; cheap tricks only suppress center-panned vocals |
| Pitch shift | Transpose the song key up/down (semitones) without changing tempo | Needs a time/pitch library; naive resampling changes tempo |
| Reverb / echo | "Venue" ambience | In a real karaoke rig this is applied to the *microphone*; OK Player has no mic input path (see §9) |

Plus the stated direction: *"Level 2: stems + sing-along"* — per-stem playback
(instrumental / vocals) with the vocal level ridable at runtime.

Cross-cutting constraints: the feature must work identically on both tracks
(Windows/libmpv and Linux/libmpv2), react at interactive latency (a pitch change
mid-song must be seamless), and survive the shipped libmpv build matrix.

## 2. Guardrails that shape the answer

**PRD.** Three lines in `docs/OK-Player-PRD.md` bound the design space:

- *"No scripting, no Lua, no plugin/extension system. Deliberately excluded to
  protect curated simplicity."* — any option whose product shape is "user loads
  plugins" is out.
- *"Do not build a dedicated audio-filter UI."* — whatever ships must be a small
  set of curated karaoke controls, not a generic filter-chain editor. Even that
  needs an explicit PRD amendment (§9).
- The `mpv.conf` escape hatch (§18.1) is the sanctioned power surface — options we
  reject as product features may still be reachable there by power users, at zero
  product cost.

**Architecture.** The repo already has the exact mechanism a curated chain needs:
a labeled mpv audio filter, added and removed at runtime, mirrored on both tracks —
`@okpnorm:dynaudnorm` in `rust/crates/okp-mpv/src/player.rs`
(`set_audio_normalization`) and `src/OkPlayer.App/Views/PlayerView.xaml.cs`.
Freeze-boundary discipline applies: any karaoke state machine, preset table, or
filter-string builder is portable domain logic and belongs in `okp-core` (shared
schema, unit-tested pure functions), with `okp-mpv` / the C# engine layer reduced
to thin `af` command helpers and the shells to UI wiring.

**Build matrix (verified 2026-07).**

| Platform | libmpv source | librubberband | LADSPA/LV2 hosting |
|---|---|---|---|
| Windows | GPL `mpv-dev-x86_64` from [zhongfly/mpv-winbuild](https://github.com/zhongfly/mpv-winbuild) (`scripts/fetch-natives.ps1`) | yes (listed in the build's dependency set) | not enabled |
| Linux | distro `libmpv2` (`.deb` Depends, `rust/packaging/scripts/package-linux-deb.sh`) | yes ([Debian libmpv2 depends on `librubberband2`](https://packages.debian.org/bookworm/libmpv2)) | LADSPA enabled in Debian's FFmpeg (`--enable-ladspa`); LV2 varies by distro build |

Both shipped builds link librubberband, so mpv's native `rubberband` audio filter —
the load-bearing piece for pitch shift — is available on both tracks today.

## 3. Option A — mpv `af` chains (native + lavfi/FFmpeg filters)

mpv exposes runtime filter management (`af add` / `af remove` on labeled entries)
and, critically, **`af-command`** for changing a running filter's parameters without
rebuilding the chain. Any FFmpeg audio filter is reachable through the automatic
`lavfi` bridge. This is the same mechanism `@okpnorm` uses.

**Pitch shift — solved.** mpv's native `rubberband` filter supports live control
via `af-command` (`set-pitch <value>`, `multiply-pitch <factor>`) per the
[mpv manual](https://github.com/mpv-player/mpv/blob/master/DOCS/man/af.rst).
A key change of *n* semitones is `pitch-scale = 2^(n/12)`; applying it via
`af-command` is glitch-free, unlike removing/re-adding the filter. Fallbacks are
strictly worse: `asetrate`+`aresample` couples pitch to tempo; `scaletempo2` is
tempo-only.

**Vocal suppression — classic "karaoke button" quality only.** Phase cancellation
of the center channel, e.g. `pan=stereo|c0=c0-c1|c1=c1-c0` or `stereotools`
mid-level suppression (exact recipe to be chosen by prototype). Removes only
center-panned, dry vocals; collateral damage to center-panned bass/kick (a
band-split variant that cancels only the vocal band can soften this); useless on
mono sources. Note: for surround content this gets *easier* — vocals live in the
center channel, which a downmix can simply drop — but karaoke material is
overwhelmingly stereo. Historical footnote: mpv had a built-in `af=karaoke` doing
exactly this trick until the old filter core was removed in mpv 0.29; the lavfi
recipes are its modern replacement.

**Reverb/echo — adequate.** `aecho` covers echo/slapback presets with no assets.
Convolution reverb via `afir` sounds better but requires shipping impulse-response
files (an asset/licensing question); defer that choice to a prototype.

**Verdict: strongest option.** Zero new dependencies, works on both tracks,
runtime-controllable, and matches an already-shipped pattern. A *fixed, curated,
app-owned* filter chain is not a plugin system — the user never loads anything —
so it is compatible with the PRD's exclusion, though the control cluster itself
still needs a PRD amendment (§9). Its one real limit is vocal-removal quality,
which no real-time filter fixes (that's what Option D2 is for).

## 4. Option B — LADSPA/LV2 plugin hosting (FFmpeg `ladspa`/`lv2` filters)

Technically feasible on Linux (Debian's FFmpeg enables LADSPA), **absent from the
Windows winbuild** — so no cross-track parity. Worse, the product shape — discover
installed third-party plugins, configure them, manage their state — is precisely
the plugin/extension system the PRD excludes, plus a runtime dependency on
whatever plugin packages the user has installed.

**Verdict: rejected.** Linux power users can already reach it through the
`mpv.conf` hatch (`af=ladspa=…`) with zero product work — that is the sanctioned
line, and this research recommends leaving it there.

## 5. Option C — VST hosting

No FFmpeg/mpv VST host filter exists. VST2 SDK licensing is dead; VST3's SDK is
GPLv3-compatible but hosting means building an entire plugin host (GUI embedding,
threading, state, crash isolation) — a project larger than the player's whole
audio path, for a Windows-centric ecosystem with poor Linux parity, and again the
excluded plugin-system product shape.

**Verdict: rejected** without further study.

## 6. Option D — external DSP

**D1: realtime audio-server routing** (PipeWire `filter-chain`, JACK/Carla).
Linux-only, configured at the system level outside the app, fragile across
distros, nothing equivalent on Windows. **Rejected as a product feature**; it
remains available to power users via audio-device selection plus their own
PipeWire config.

**D2: offline stem separation — the only path to real vocal removal.** ML source
separation (Demucs/htdemucs, MDX-Net/UVR-class models) actually isolates vocals,
and is the substance behind *"Level 2: stems + sing-along"*: separate once in a
background job, store stems as **sidecar files next to the media** (fitting the
existing sidecar persistence philosophy), then play the original video with stems
as external audio mixed via `lavfi-complex` (`amix` with per-input gain) so the
vocal level is ridable at runtime — full removal, quiet "guide vocal", or full mix.

Costs are real and unresolved: shipping or downloading an inference runtime
(PyTorch/ONNX, hundreds of MB) and model weights (license review required per
model — Demucs code is MIT, weight licenses vary); separation is minutes per track
on CPU; disk cost per separated track; job-queue UX. **Verdict: promising but its
own project** — it needs a dedicated design doc before any commitment, and Tier 1
below does not depend on it.

## 7. Recommendation

Two tiers, independently shippable:

- **Tier 1 — curated karaoke chain on Option A.** Three fixed, app-owned labeled
  filter slots (following the `@okpnorm` naming pattern): vocal suppression
  (lavfi phase-cancel recipe), pitch (`rubberband`, semitone steps driven by
  `af-command`), ambience (`aecho` presets). A handful of curated controls — not a
  filter UI, no user-supplied chains. This is the cheapest credible karaoke mode
  and every piece is verified available on both shipped builds.
- **Tier 2 — offline stems (Option D2)** as a separate follow-up research/design
  issue; it supersedes Tier 1's vocal suppression when stems exist for a track and
  is the real "Level 2" feature.
- **Rejected:** LADSPA/LV2 hosting (B — parity + PRD), VST hosting (C — cost +
  licensing + PRD), realtime external DSP (D1 — Linux-only, out-of-app).

## 8. Where the code would live (if green-lit)

- `okp-core`: karaoke state + preset tables + pure filter-string builders
  (semitone → `pitch-scale` math, recipe strings), unit-tested; shared JSON schema
  for persisted karaoke preferences.
- `okp-mpv` / C# engine layer: thin helpers in the shape of
  `set_audio_normalization` (`af add`/`af remove`/`af-command` on the labeled
  slots).
- `okp-linux-gtk` / WinUI shells: UI wiring only (freeze-boundary).
- Tests: unit tests on the builders in `okp-core`; one integration test per track
  asserting `af add` of each curated filter succeeds against the real libmpv —
  this turns the §2 build-matrix table into a CI-enforced invariant instead of a
  research-time snapshot.

## 9. Open questions (blockers before any implementation issue)

1. **PRD sign-off.** A karaoke control cluster needs an explicit amendment to the
   "no dedicated audio-filter UI" line — curated controls, fixed effects, no
   generic chains. Owner decision, not an engineering one.
2. **What did the tester mean by reverb?** Karaoke venues apply reverb to the
   *microphone*; OK Player has no mic input, so we can only add ambience to the
   program audio. Worth a follow-up before building the third slot at all — it may
   be a mic-path feature request in disguise, which is far out of scope.
3. **Vocal-suppression recipe** — `pan` vs `stereotools` vs a band-split variant:
   pick by listening test on real karaoke material (prototype, §10).
4. **`afir` IR assets** — only if convolution reverb wins over `aecho`; shipping
   IR files raises size and licensing questions.
5. **Tier 2 packaging** — inference runtime distribution, model weight licensing
   vs GPLv3 shipping, CPU-only separation time; belongs to the follow-up design
   doc.

## 10. Appendix — prototype one-liners

Manual listening tests against a stock `mpv` on either platform:

```sh
# Vocal suppression (phase cancel)
mpv --af=lavfi=[pan=stereo|c0=c0-c1|c1=c1-c0] song.mkv

# Pitch shift +2 semitones, live-tunable
mpv --af=@pitch:rubberband=pitch-scale=1.12246 song.mkv
#   at runtime: af-command @pitch multiply-pitch 1.05946   (up one more semitone)

# Echo preset
mpv --af=lavfi=[aecho=0.8:0.7:60:0.3] song.mkv
```

## References

- [mpv manual — audio filters (`rubberband`, `af-command`)](https://github.com/mpv-player/mpv/blob/master/DOCS/man/af.rst)
- [zhongfly/mpv-winbuild](https://github.com/zhongfly/mpv-winbuild) — Windows libmpv source (see `scripts/fetch-natives.ps1`)
- [Debian `libmpv2` package](https://packages.debian.org/bookworm/libmpv2) — Linux runtime dependency set
- [FFmpeg `ladspa`/`lv2` filters](https://ffmpeg.org/ffmpeg-filters.html)
- [Demucs (music source separation)](https://github.com/adefossez/demucs)
- `docs/OK-Player-PRD.md` — plugin-system exclusion, audio-filter UI exclusion, `mpv.conf` hatch
