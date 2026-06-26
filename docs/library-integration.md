# Library Integration — Report-Back Surface

This documents the **concrete shape** of the player → companion-library progress
report-back contract. It is the implementation reference for the MVP contract in
[`OK-Player-PRD.md` §13](OK-Player-PRD.md#13-companion-library-integration--storage-model);
the PRD is the source of truth for *what* the contract guarantees, this file pins
*how* it is currently realized. The PRD deliberately leaves "report-back
cadence/channel … an implementation choice (CLI/callback/local IPC); keep it
pluggable" (§13.1) — so treat the format below as the current realization, not a
frozen API.

## Current surface: the app-index JSON

Today the player reports progress by writing its per-file playback memory to the
human-readable app index (PRD §13.3). A companion app reads this file to learn
what has been watched and how far.

- **Location:** `%APPDATA%\OkPlayer\history.json`
- **Format:** a single JSON object keyed by the media file's **full path**
  (case-insensitive). Written crash-safe (atomic temp-then-rename), so a reader
  that opens it mid-write sees either the old or the new complete file, never a
  truncated one.
- **Scope:** local files only. Network streams / URLs are not tracked.
- **Privacy:** while a private (incognito) session is active, no new entries are
  written and existing ones are not updated — a reader should treat absence/staleness
  as "no signal," not "not watched."

### Record schema

```jsonc
{
  "C:\\media\\show\\s01e03.mkv": {
    "Position": 1342.5,            // resume point in seconds; 0 once Finished
    "Duration": 2640.0,            // total runtime in seconds (0 if never determined)
    "Finished": false,             // true once watched past the near-end threshold
    "LastOpenedUtc": "2026-06-26T20:14:07.1234567Z", // ISO-8601 round-trip (UTC)
    "Title": "s01e03",            // filename without extension
    "PosterPath": "…",            // optional cached poster frame (may be absent)
    "Bookmarks": [120.0, 845.5],   // optional; user bookmarks in seconds
    "UserChapters": [ { "Time": 60.0, "Title": "Intro" } ] // optional
  }
}
```

Fields a reader can rely on for watched-state and progress:

| Want | Read |
|---|---|
| Percent watched | `Finished ? 1.0 : (Duration > 0 ? Position / Duration : 0)` |
| "Resume from here" point | `Position` (seconds) |
| Has it been finished? | `Finished == true` |
| Last activity | `LastOpenedUtc` |

### Why `Finished` exists

`Position` is reset to `0` when the playhead is parked in the file's final stretch,
so it neither auto-resumes nor lingers half-watched in continue-watching. That makes
`Position` alone ambiguous: `0` means **either** "finished" **or** "never started."
The `Finished` flag disambiguates — it is the "watched flag when the near-end
threshold is crossed" the PRD calls for (§13.1).

`Finished` latches only when the file **plays through to a natural end-of-file**, not
from a sampled position — so seeking into the final stretch, or seeking back after the
credits, does not flip it, and a clip never reads as finished merely by being opened.
Re-watching a completed file from the start (it reopens at position 0) clears the flag
once playback progresses.

## Cadence

Progress is written periodically while playing and once more when the file is
closed/replaced. There is no push/callback channel yet — a companion app polls or
watches the file. A future revision may add a push channel (CLI callback / local
IPC / `ok-player://`) without changing this schema.

## Inbound: launch-with-resume

The other half of the PRD §13.1 contract. The library launches the player with a
file and an explicit position to start at:

```
OkPlayer.exe "C:\media\show\s01e03.mkv" --resume 1342
OkPlayer.exe "C:\media\show\s01e03.mkv" --resume=22:22   # m:ss / h:mm:ss also accepted
```

- `--resume <time>` (also `--resume=<time>` / `--resume:<time>`, `-resume`, `/resume`)
  takes seconds (`1342`, `1342.5`) or a timecode (`22:22`, `1:23:45`).
- The given position is honored **verbatim**: it overrides the player's own
  remembered position and bypasses the auto-resume heuristic (the < 5% / last-30s
  skip), because in this flow the library — not the player — decides where to start.
  Application waits until the media's duration is known to cover the target (mpv may
  report a provisional duration first for network/progressive media); a value beyond
  the media's end is ignored and the file starts from the beginning.
- `--resume 0` is meaningful: "start from the beginning," overriding a remembered
  position.
- A malformed or missing value is ignored — the player falls back to its own
  remembered position.

### Track preselection

The optional `[sub=…, audio=…]` in the PRD diagram — preselect a subtitle/audio
track on launch:

```
OkPlayer.exe "C:\media\show\s01e03.mkv" --resume 1342 --sub 2 --audio 1
OkPlayer.exe "C:\media\show\s01e03.mkv" --sub no          # start with subtitles off
```

- `--sub <id>` / `--audio <id>` take an **mpv track id** (a positive integer — mpv
  track ids are 1-based; `0` is ignored, since mpv reads it as "auto"). Ids are as
  listed in the track switcher / `track-list`. Also `--sub=<id>` / `--sub:<id>`,
  `-sub`, `/sub`.
- `--sub no` / `--sub off` (and the audio equivalents) explicitly select **none**.
- Applied as mpv selects the track on load; a malformed/missing value is ignored.
- Track ids are file-specific — the library is expected to pass an id it learned from
  a prior probe of the same file.
