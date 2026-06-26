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

## Not yet implemented (inbound half)

The other half of the PRD §13.1 contract — **launch-with-resume** (the library
launching the player with an explicit "resume from X" that overrides remembered
position) — is not wired up yet. When added it will be documented here.
