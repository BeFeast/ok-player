# Session ABI (`okp_session_*`)

The native shells drive playback through one session object from the
`okp-session` crate. `okp-ffi` exports it as a small C ABI that carries JSON
documents. It is built only with the opt-in `live-mpv` feature; the default
`okp-ffi` build stays engine-independent.

The session decides and the shell renders: every command is validated by the
core player state machine before anything reaches libmpv, and the shell's view
of the player comes only from what the session reports.

## Functions

```c
OkpSession *okp_session_new(const char *config_json, char *error_buffer, uintptr_t error_capacity);
void        okp_session_free(OkpSession *session);
char       *okp_session_dispatch(const OkpSession *session, const char *command_json);
char       *okp_session_query(const OkpSession *session, const char *query_json);
void        okp_session_set_wakeup(const OkpSession *session, OkpSessionWakeup callback, void *ctx);
char       *okp_session_drain_events(const OkpSession *session);
void        okp_string_free(char *text);

bool        okp_session_render_create(const OkpSession *session, OkpSessionRenderUpdate update, void *update_ctx,
                                      char *error_buffer, uintptr_t error_capacity);
bool        okp_session_render_update(const OkpSession *session);
bool        okp_session_render(const OkpSession *session, int32_t width, int32_t height);
uint64_t    okp_session_render_report_swap(const OkpSession *session);
void        okp_session_render_destroy(const OkpSession *session);
```

The render calls draw video frames into the framebuffer bound in the shell's
OpenGL context; see Rendering and Threading below.

## Rendering

The render context is created with libmpv's advanced control, and the shell's
`update` callback is installed before `okp_session_render_create` returns
(a null callback is refused). One frame goes:

1. `update(update_ctx)` fires on an engine thread. It only wakes the shell's
   render thread.
2. The render thread calls `okp_session_render_update`. That also serves the
   work libmpv queues for the render thread, so it is called after every
   callback, even when nothing will be drawn. It returns true when a new frame
   is due; otherwise nothing is drawn.
3. When the shell is ready to present (macOS paces this with the display
   link), `okp_session_render` draws the frame into the bound framebuffer.
4. After the shell's flush, `okp_session_render_report_swap` tells libmpv the
   frame was presented, so it can time the next one. It returns the number of
   swaps reported to the render context so far (0 without one), so the shell
   can check that every frame it presented reached libmpv. Once libmpv has
   seen one swap it waits for a swap after every frame it presents, so a shell
   that stops reporting them drops to a few frames per second.

A shell may also redraw the current frame without a callback, after a resize
for instance: `okp_session_render`, flush, `okp_session_render_report_swap`.
While paused or idle libmpv announces no frames, so a shell that draws only
due frames draws nothing.

## Documents

All text is UTF-8 and nul-terminated. Every document is an envelope carrying
`schema_version` (currently `1`). A document with another version is refused
with `unsupported_schema_version`; one without a version, with invalid JSON, an
unknown command or an unknown field is refused with `malformed`.

Enums are externally tagged with `snake_case` names, and every variant is an
object, empty or not:

```json
{"schema_version":1,"command":{"close":{}}}
{"schema_version":1,"command":{"seek":{"seconds":-5.0,"mode":"relative","precision":"keyframe"}}}
```

This is the shape Swift's synthesised `Codable` uses for enums with labelled
associated values (decode with `.convertFromSnakeCase`). Optional values are
written as `null` and may be left out on input.

| Document | Shape |
|---|---|
| Config (`okp_session_new`) | `schema_version`, `app` (`name`, `platform`), absolute `data_dir`, `config_dir`, `cache_dir`, optional `hwdec`, ordered `mpv_options` (`name`, `value`) applied after the session defaults, optional `shell_commands` (see Command registry) |
| Command (`okp_session_dispatch`) | `{"schema_version":1,"command":{...}}`: `open`, `close`, `set_paused`, `toggle_pause`, `seek`, `set_volume`, `set_mute`, `set_speed`, `select_track` (`id: null` turns the kind off), `set_subtitle_delay`, `invoke_command {id}`, `playlist` (see Playlist), `nudge_volume` and `toggle_mute` (see Transport controls), `add_subtitle`, `set_subtitle_scale` and `nudge_subtitle_scale` (see Tracks and subtitles) |
| Reply | `{"schema_version":1,"result":{...}}`: `accepted {request_id}`, `no_op {}`, `rejected {reason, message}`, `failed {request_id, message}` when the engine refused an accepted command at once, `perform {}` (`invoke_command` only), or `applied {}` when the session carried the command out itself and nothing reached libmpv yet |
| Query (`okp_session_query`) | `{"schema_version":1,"query":{...}}`: `snapshot {}`, answered with `result.snapshot.snapshot` (the full state); `command_registry {fullscreen, mini_player}` (both optional), answered with `result.command_registry.commands`; `shortcut_table {}`, answered with `result.shortcut_table.shortcuts`; `repeating_shortcuts {}`, answered with `result.repeating_shortcuts.actions`; `playlist {}`, answered with `result.playlist.playlist`; `transport {at}` (`at` optional), answered with `result.transport.transport` (see Transport controls); `tracks {}`, answered with `result.tracks.tracks` (see Tracks and subtitles); `window_fit`, `initial_window_fit` and `video_click` (see Window queries); `open_formats {}` and `open_paths {paths}` (see Opening files); or `result.rejected`; `history` queries are below |
| Event batch (`okp_session_drain_events`) | `{"schema_version":1,"events":[...]}` |

An open source is `{"kind":"local"|"url","location":"..."}`. A URL is handed
to libmpv as it is; nothing resolves it yet. `resume_from` is where playback of
that file starts. `audio_track` and `subtitle_track` are selected once the file
has loaded, and only if it has a track of that kind and id; otherwise libmpv's
own choice stands. Track choices and the subtitle delay belong to one file:
the next open starts from libmpv's defaults again, whatever was selected or set
while the previous file played.

A position before the start of the media (a negative `resume_from` or absolute
`seek`) means the start.

The snapshot carries `status`, `source`, `time_pos`, `duration`, `volume`,
`mute`, `speed`, `subtitle_delay`, `tracks`, `chapters`, `end_reason`, `title`,
`video` (`width`, `height`, `display_aspect`), `buffering` and `cache_ahead`.

The golden documents in `rust/crates/okp-session/fixtures/` (the playlist's
are the `playlist-*.json` ones) pin every shape and every enum value. Change
them only together with the protocol. The watch history's are in its
`history/` subdirectory; a shell test that checks it covers every fixture in
the directory and below notices a new document.

## Compatibility within a version

Within one `schema_version` the protocol only grows. A new command, query or
event, or a reply variant that answers only a new command, keeps the version.
A shell built before the addition keeps working because it follows two rules:

- It skips an event it does not know. Each event is its own entry of
  `events`, so the shell decodes the entries one by one and drops an entry
  whose name it does not know; it never discards the rest of the batch. Swift's
  synthesised `Codable` fails a whole `[Event]` on an unknown case, so decode
  each entry on its own. For example, a session that has the playlist sends
  `playlist_changed` after an `open`, whether or not the shell uses the
  playlist.
- It gets new reply and query-result variants only for the commands and queries
  it sends: `applied` answers only playlist commands, and `result.playlist`
  answers only the playlist query.

Everything else bumps `schema_version`: a new, renamed or removed field or enum
value in an existing document, a new snapshot field, or a changed meaning. A
session refuses documents of another version, so a shell and a session with
different versions do not talk at all.

## Window queries

Three queries answer window questions with okp-core's rules, so every shell
sizes its window and reads clicks on the video the same way.

- `window_fit {video, backing_scale, visible_frame, min_size, chrome}` gives the
  frame of a window fitted to a video: `result.window_fit.frame`
  (`{x, y, width, height}`), or `null` when there is nothing to fit. `video` is
  the snapshot's video size in pixels, `backing_scale` the screen's device
  pixels per logical unit, `visible_frame` the part of the screen windows may
  use, the optional `min_size` the window's minimum, and the optional `chrome`
  the room the window's own controls take outside the video area (`{width,
  height}`: a bar below the video, a panel beside it). The video is shown at no
  more than one video pixel per device pixel, inside core's desktop margin and
  title-strip reserve, fitted to the screen less `chrome`; the frame is that
  size plus `chrome`, centered in `visible_frame`, so the video area itself has
  the video's aspect. A fit below `min_size` grows to it with the video area's
  aspect. `x`, `y` is the corner with the smallest coordinates, so the same
  numbers work for y-down desktops and for AppKit's bottom-left screen
  coordinates. The frame is the window's content; the shell's title strip lies
  over the video. It reads no session state.
- `initial_window_fit {backing_scale, visible_frame, min_size, chrome}` gives
  the fit the current source is owed, as `result.window_fit.frame`: once per
  opened source, the `window_fit` frame for the first video size the snapshot
  reports while that source is loaded. Answering takes the fit; every other
  answer is `null`: before the size is known, after the fit was taken, for a
  source without video, and for a fit still owed to a source that a later open
  or a close replaced (core `InitialFitState`, keyed by a source generation the
  session starts on every accepted `open` and `close`). A shell can ask after
  every batch with a video size; only the first answer for a source resizes
  the window. A shell in fullscreen asks too and drops the frame, so a later
  exit from fullscreen is not refitted.
- `video_click {press_count}` says what a primary-button release with that
  click count on the video means: `result.video_click.intent` is
  `schedule_play_pause` (toggle pause once the double-click interval passes
  without a second click), `cancel_play_pause_and_toggle_fullscreen` (drop that
  pending toggle and toggle fullscreen) or `ignore`. The shell owns the timer.
  It reads no session state.

## Command registry

The registry is everything a menu or a key can run: the core player command
registry (the commands of the Linux command menu, with their ids and labels)
followed by the bindable keyboard actions no registry command covers
(`seek-back`, `volume-up`, `mute`, `next-item`, the subtitle nudges, ...),
under their shortcut action ids. Each `command_registry` row carries `id`,
`group`, `menu` (the place in the Linux command menu, `null` for a
keyboard-only command), `label`, `available`, `enabled`, `checked` and
`shortcut`, the primary chord bound to it on the config's platform.
`enabled`, `checked` and the state-dependent labels ("Mute" checked while
muted, that is while libmpv's mute flag is on or the level is 0, as the
transport view's `muted`; "Exit fullscreen") are resolved against the session state at the time
of the query, the session playlist included: `next-item` and `previous-item`
are enabled while it holds two items or more. The window state is the shell's, so it passes `fullscreen` and
`mini_player` in the query. Query when a menu opens; no event announces a
change.

The session runs some commands itself: `play-pause`, `close-media`,
`seek-back`/`seek-forward` (5 s), `volume-down`/`volume-up` (a
`nudge_volume` of 5 points), `mute` (a `toggle_mute`),
`subtitle-delay-back`/`subtitle-delay-forward` (0.05 s, from the delay asked
for last), `subtitle-size-down`/`subtitle-size-up` (0.1, as
`nudge_subtitle_scale`). The volume steps and mute go through the same volume
model as the transport controls (see Transport controls): a step builds on the
level asked for last, and `mute` unmutes a mute made from the controls and the
other way round. For these, `enabled` is exactly when the session accepts the
command:
`play-pause`, the seeks and the subtitle nudges while the status is `playing`
or `paused` (not while a file opens, after it ends or after a load error,
although the source stays set), `close-media` while the status is not `idle`,
and volume and mute always. The rest are carried out by the shell (dialogs,
window, clipboard). A shell lists the ones it implements in the config's
`shell_commands`; a command that neither the session nor the shell carries
out is `available: false`, and the shell should hide it. An id in
`shell_commands` that is not in the registry fails `okp_session_new`.

`invoke_command {id}` answers `rejected` with `unknown_command` for an id
outside the registry and `unavailable` for an unavailable command. A command
the session runs is dispatched as the session command it stands for and
answered like one (`accepted`, `no_op`, `rejected`, `failed`). A command the
shell carries out is answered `perform {}`: the shell runs it, gating it on
the `enabled` state it queried.

`shortcut_table` rows carry `action`, `command` (the registry id to invoke),
`primary` and `secondary`. The session has no user bindings yet, so these are
the platform defaults and `secondary` is `null`. A chord is
`{"key", "modifiers": {"ctrl", "alt", "shift", "cmd"}, "label"}`: `key` is
the canonical key name (`space`, `comma`, `Left`, `o`) and `label` the display
text (`Shift+Cmd+O`). On macOS menu commands use Cmd chords and in-window keys
stay single keys.

`repeating_shortcuts` lists the actions whose key repeats while held (the
seek, frame, volume and subtitle delay, size and cue steps): each auto-repeat
runs the action again. Every other action runs once per physical press (core
`key_press`), so a held Space, M or F toggles once. The list is pinned by
`query-reply-repeating-shortcuts.json`.

## Transport controls

The `transport` query answers what the transport controls show for the
current snapshot, computed with the core's rules so a shell does not repeat
them. A shell asks for it after each drained batch.

| Field | Meaning (core rule) |
|---|---|
| `elapsed` | `MM:SS`, or `HH:MM:SS` from the first hour (`time_code::format_clock`) |
| `remaining`, `total` | the trailing label in either mode; `--:--` for a URL without a known duration, `-00:00` / `00:00` for a local file still loading (`time_code::format_trailing`) |
| `progress` | the playhead as a fraction of the duration, 0 while it is unknown |
| `buffered` | how far the cache reaches, as a fraction (`timeline_buffer::fraction`) |
| `chapter_ticks` | chapter starts as fractions (`chapter_math::fractions`) |
| `seekable` | media is loaded and its duration known |
| `volume` | `level`, `max` (130), `fraction`, `unity_fraction` (100 %), `muted` (muted or 0 %), `boosted`, `readout` (`Muted`, `85%`, `54.7%`) (`volume`) |
| `speed`, `speeds` | the current speed as `1.00×` and the menu choices 0.5, 0.75, 1, 1.25, 1.5 and 2× with `selected` |
| `osc` | `revealed` and `hidden`: the controls' `visible`, `mapped`, `focusable` and `hit_testable` while shown and once hidden for inactivity (`osc_visibility::project`); `auto_hide`: they hide after `hide_after_ms` (2500, PRD P1-D6) without activity, only while media plays |

With `"at": <seconds>` the time readout and `progress` are projected at that
position instead of the playhead, which is what a scrubber shows while it is
dragged; `buffered` stays with the playhead.

`{"nudge_volume":{"delta":<percent>}}` changes the volume relative to the
level the session asked libmpv for last, or the reported level when libmpv has
caught up, clamped to 0-130 % (`volume::VolumeState::nudge`). It is forwarded
as a `set_volume` and answered like one. libmpv reports volume changes
asynchronously, so nudges that arrive faster than its reports (a scroll wheel)
all count instead of each building on the same stale report
(`volume::reconcile_observed_level`).

`{"toggle_mute":{}}` mutes the way the core and the Linux player do: mute is a
level of 0 that remembers the last level above it, and the next toggle goes
back there (`volume::VolumeState::toggle_mute`). The remembered level is the
last level above 0 that was asked for or reported, so a slider dragged to 0 %
or a level nudged down to 0 is unmuted by the same toggle, and a nudge or a
`set_volume` above 0 is audible again. It is forwarded as a `set_volume` and
built on the level asked for last, like a nudge. `set_mute` is libmpv's
separate mute flag, which leaves the level alone; while it is on, `toggle_mute`
is forwarded as `set_mute(false)` instead, so the first toggle always clears it.

The documents are pinned by `rust/crates/okp-session/fixtures/transport/`.

## Tracks and subtitles

The `tracks` query answers what the audio and subtitle menus show, with the
core's rules, so a shell neither labels a track nor formats a readout. A shell
asks for it when a menu opens, after each drained batch while one is shown,
and after it changed the delay or the size.

| Field | Meaning (core rule) |
|---|---|
| `audio` | the audio tracks in libmpv's order, each `{id, label, selected}`; empty without loaded media |
| `subtitles` | `Off` (`id: null`, selected while no subtitle track is) first, then the subtitle tracks the same way |
| `label` | the one-line descriptor: name (title, else language, else `Track N`), language, channels and codec for audio; format, `Image`, `Native style` and `EXT` or `Default` for subtitles (`track_label`) |
| `subtitle_delay` | `{value, label, reported}`: seconds and `+250 ms` / `0 ms` (`subtitle_delay::format_label`) |
| `subtitle_scale` | `{value, label, reported}`: the size factor and `110%` |
| `adjustable` | media is `playing` or `paused`, so a file can be added and the delay and size changed |

A row's `id` goes to `select_track` with the menu's kind; `null` turns subtitles
off.

- `{"add_subtitle":{"path":"/…/film.en.srt"}}` loads a subtitle file for the
  current media and selects it (`sub-add … select`). A path whose extension is
  not a subtitle's (`.srt`, `.ass`, `.ssa`, `.sub`, `.vtt`, `.idx`, `.sup`) is
  `rejected` with `invalid_argument`; without loaded media it is `rejected`
  with `no_active_media`. libmpv reads the file after the reply: a file it
  cannot read is reported by an `error` event (`command_failed`) with the
  command's `request_id`.
- `{"set_subtitle_scale":{"scale":1.2}}` sets the subtitle size, kept within
  0.25-4 (`subtitle_style::normalized_scale`). `{"nudge_subtitle_scale":{"delta":0.1}}`
  changes it from the size asked for last, or the reported one, to the nearest
  hundredth, and is forwarded as a `set_subtitle_scale`. Both need loaded media
  and a finite value. The size holds across files and is not part of the
  snapshot: `tracks` reports it.
- Subtitle files named after the media load with it: `movie.srt`,
  `movie.en.srt` and the other languages next to `movie.mkv` (libmpv
  `sub-auto=exact`, which okp-mpv sets). They are listed as external tracks.

libmpv reports the delay and the size asynchronously. Until it does, the
values asked for lead: delay and size keys pressed faster than its reports all
count, and `tracks` reads the new value at once in `value` and `label`.
`reported` is libmpv's own last report (`null` before its first), so a shell or
a test can tell when a change has reached libmpv. The snapshot's
`subtitle_delay` follows when libmpv reports it.

The documents are pinned by `rust/crates/okp-session/fixtures/tracks/`.

## Events

A batch holds at most one `snapshot_changed` event, first, and then at most one
`playlist_changed` event carrying the whole playlist as it is at the drain. The
`snapshot_changed` event's `changes` list
the latest value of every snapshot field that changed since the previous batch,
in snapshot order, each as `{"<field>":{"value":...}}`. A field that changed
several times between drains appears once. Applying the changes in order to the
snapshot from a query gives the session's state as of the drain.

`error` events (`kind`, `message`, `request_id` when the error answers a
command) follow in the order they happened, and `shutdown` comes last. The
kinds are `load_failed`, `command_failed`, `engine`, and `nothing_playable`
(see Playlist).

`time_pos` and `cache_ahead` change on every frame. On their own they are
published at most about ten times a second; whenever another field changes they
are published with it at once.

## Playlist

The session owns the playlist: Up Next, what plays after the current item, and
the edits a shell offers. A playlist command is
`{"schema_version":1,"command":{"playlist":{"action":{...}}}}`; indexes count
from 0 in the order of the playlist's `items`.

| Action | Effect | Reply |
|---|---|---|
| `open {sources}` | replaces the playlist; see Opening below | the open's reply, `applied` while folders or a playlist file are read, `no_op` when nothing is playable |
| `next {}`, `previous {}` | one step in play order, wrapping at both ends whatever the repeat mode | the open's reply; `no_op` with fewer than two items |
| `jump {index}` | plays that item (the current one restarts) | the open's reply; `no_op` out of range |
| `queue {sources, mode}` | `append` or `play_next` around the current item; only media files and plain `http`/`https` URLs, each once | `applied`; `no_op` when nothing new is queueable; `rejected`/`no_active_media` without media |
| `play_next {index}`, `move {from, to}`, `remove {index}`, `clear_queue {}` | queue edits; the current item is never removed | `applied`, or `no_op` when nothing changed |
| `set_repeat {mode}` | `off`, `one` or `all` | `applied` or `no_op` |
| `set_shuffle {enabled, seed}` | shuffles the play order with the current item first; a `seed` makes it reproducible | `applied` or `no_op` |

An item the playlist plays is opened exactly like an `open` command, so the
reply and the snapshot are the same as for one. `applied` means the session
carried the command out itself and nothing reached libmpv yet.

Opening:

- One local media file, through `open` or a playlist `open`, plays at once and
  is alone in the playlist while its folder is listed on a scan thread. When
  the listing finishes, the folder's media (by extension) fill in, in natural
  order of their file names, with anything queued meanwhile after them.
- Several sources play in the given order, without repeats; a folder expands to
  its media in natural order. A subtitle is not media. An `.m3u`/`.m3u8` file
  counts only when nothing else is media: its entries play in document order,
  relative entries resolved against the playlist file's folder.
- Folders and playlist files are read on a scan thread too. The reply is
  `applied`; the first item opens when the read finishes, and an `error` event
  (`nothing_playable`, no `request_id`) reports a read that found nothing
  playable. Whatever was loaded before goes on playing, so a shell reports it
  without treating the current media as failed.
- While an open's folders and playlist files are read, the playlist it
  replaces stays. `jump`, `next` and `previous` play from it and drop the
  read. Sources queued meanwhile show in it at once and follow the opened
  items when the read finishes, unless they were removed again. `play_next`,
  `move`, `remove` and `clear_queue` change only the playlist being replaced;
  repeat and shuffle carry over.
- A newer open or a `close` supersedes a read in flight: it is cancelled, and
  whatever it reports is dropped. A listing reads at most 20,000 directory
  entries; a bigger folder is not listed.
- A URL source is played as given; nothing resolves it yet.

When the current item reaches its end without an error, the next item in play
order plays (`repeat` `all` wraps to the first), and `one` restarts the same
item. An end while a folder or playlist file is being read waits for the read:
after a folder listing, the next item comes from the filled-in playlist; after
an open's read, its first item plays, or the old playlist goes on if the read
found nothing playable. An open or a `close` in the meantime cancels the
advance. A plain `open` of anything other than a local media file makes a
one-item playlist (libmpv is handed the source as it is, so folders and
playlist files belong in a playlist `open`); `close` empties it.

The query `{"schema_version":1,"query":{"playlist":{}}}` answers
`result.playlist.playlist`: `items` (`source`, `title`), `current_index`,
`repeat`, `shuffle`, and `scanning` (a folder or playlist file is being read).

## Opening files

Two queries tell a shell what to offer and what opened files do, with
okp-core's `media_formats` rules, so a shell keeps no extension lists. Neither
reads the filesystem.

- `open_formats {}` answers `result.open_formats.formats`: `video`, `audio`,
  `subtitle` and `playlist` (`m3u`, `m3u8`), each a list of lowercase
  extensions without the dot, for an open panel's filter.
- `open_paths {paths, folders}` answers `result.open_paths.plan` for local
  paths from an open panel, a drop onto the window or the system's open
  events. `folders` (optional, empty when left out) lists those of `paths` the
  shell knows are folders, from their file URLs; the session cannot tell
  without reading the filesystem.
  - `open {sources}`: the media files (by okp-core's rules), M3U files and
    folders among the paths, as local sources in the given order and each once.
    The shell sends them as a playlist `open` (see Playlist), which plays media
    files, lists folders and reads M3U files on its scan thread. Subtitle files
    and files of any other kind among them are left out.
  - `add_subtitles {paths}`: no media file, M3U file or folder was given, but
    subtitle files were, and media is `playing` or `paused`: they belong to that
    media.
  - `nothing {}`: nothing of the above. A document, an archive or a path not
    known as a folder opens nothing, so it never replaces what plays.

The documents are pinned by `rust/crates/okp-session/fixtures/open/`.

## Watch history

The session keeps the watch history in `history.json` under `data_dir`, in the
same document the Linux build writes (`okp_core::history::History`), so the
file moves between the two. Writes go to a temporary file that is flushed and
renamed over the old one; a shell never touches the file. The session writes
it on a thread of its own, so `dispatch`, `query` and `drain_events` never
wait for the disk. Two calls do: `okp_session_free`, which waits for the last
save, and `history.remove`, which waits for its own write to answer.

What the session does on its own:

- An `open` without `resume_from` starts at the remembered position when the
  core resume rule allows it: past the first 5%, before the last 30 s (the last
  5% for media under 10 minutes), and not finished. Once that media has loaded,
  a `resumed` event reports the position. A `resume_from` in the open wins.
- Progress is saved every 10 s while playing, and on pause, `close`, the next
  `open`, `okp_session_free` and the engine quitting. Playing to the end marks
  the entry finished, so it opens from the start next time.
- `select_track`, `set_subtitle_delay`, `set_volume` and `set_speed` are
  remembered for the current media and written with the next save, or 10 s
  after the first unsaved one, so a dragged slider writes once. Reopening it
  sets the volume and speed with the open, and selects the tracks (only those
  the file has) and sets the subtitle delay once it has loaded; tracks named in
  the open win.
- While the private session is on, nothing new is recorded and nothing
  remembered is applied. The history stays readable and entries can still be
  removed. It starts off with every session.
- A `history.json` that is there but cannot be read (damaged, written by a
  newer build, or unreadable when the session starts) is never written over.
  The first save renames it to `history.json.unreadable-<unix seconds>` and
  then writes a new file; if the rename fails, nothing is written and
  `save_failed` follows. The page reports `read_failed` and, once the file has
  been moved, its new path in `unreadable_copy`.

| Document | Shape |
|---|---|
| Command | `{"command":{"history":{"action":{...}}}}`: `set_private {enabled}`, `remove {source}` (the media is untouched; removing the media that plays stops recording it until it is opened again) |
| Reply | `{"result":{"history":{"result":{...}}}}`: `done {}`, `unchanged {}` or `failed {message}` (the file could not be written; nothing changed) |
| Query | `{"query":{"history":{"query":{...}}}}`: `welcome_shelf {limit, utc_offset_seconds}`, `page {search, utc_offset_seconds}` (`search` may be left out) |
| Query reply | `{"result":{"history":{"result":{...}}}}`: `welcome_shelf {private_session, entries}` or `page {private_session, read_failed, unreadable_copy, summary, end_cap, sections}`; a section is `{bucket, header, entries}`, newest first |
| Event | `{"history":{"event":{...}}}`: `resumed {position}`, `changed {}` (query again), `private_session {enabled}`, `save_failed {message}` (once until a save succeeds) |

An entry carries `source`, `title`, `location` (the folder breadcrumb),
`position`, `duration`, `progress` (0 to 1), `state` (`finished`, `progress`,
`barely`), `state_label`, `when_label`, `updated_at_unix` and `resume_from`
(where opening it starts, `null` for the start). Days and `when` labels are
local to the `utc_offset_seconds` the shell passes: its current offset, applied
to every entry. Entries for local files that no longer exist are left out.

History events are discrete: they keep their order among the `error` events.
The golden documents for this group are in
`rust/crates/okp-session/fixtures/history/`.

## Threading and ownership

- Folder listings and playlist-file reads run on short-lived scan threads of
  their own, never on a caller's thread or the event thread. A scan thread only
  hands its result to the event thread; one stuck on a hung mount is abandoned.
- The session runs its own event thread. The wakeup is called on that thread,
  once per batch: after it fires it fires again only when
  `okp_session_drain_events` has taken the batch. The wakeup should only
  schedule a drain on the shell's main thread. It may call
  `okp_session_drain_events`, `okp_session_dispatch` and `okp_session_query`,
  but never `okp_session_set_wakeup` or `okp_session_free`.
- If events are already waiting when a wakeup is installed, it fires promptly.
- When `okp_session_set_wakeup` returns, the replaced wakeup is not running
  and is never called again, so its `ctx` may be released. `ctx` must be usable
  from the event thread while it is installed.
- `okp_session_dispatch`, `okp_session_query` and `okp_session_drain_events`
  may be called from any thread.
- The render calls belong to the shell's render thread, with the OpenGL
  context they draw into current: `okp_session_render_create` once that
  context exists (a repeated call does nothing), `okp_session_render_update`,
  `okp_session_render` and `okp_session_render_report_swap` per frame, and
  `okp_session_render_destroy` before the context goes away and before
  `okp_session_free`. They may run while other threads dispatch, query and
  drain. `okp_session_render` returns false when there is no render context or
  libmpv failed to draw.
- Advanced control binds the render thread: it must never wait for a thread
  that is inside another `okp_session_*` call (libmpv may be waiting for the
  render thread at that moment), and it must call `okp_session_render_update`
  promptly after every `update` callback. The callback itself must not call
  any `okp_session_*` function.
- `update_ctx` must be usable from an engine thread until
  `okp_session_render_destroy` returns; after that the callback never runs
  again.
- `okp_session_free` discards undrained events, stops the event thread and
  shuts libmpv down. When it returns the wakeup is not running and never runs
  again. It must not race any other call on the same session.
- Every `char *` returned by the session is owned by the caller and released
  with `okp_string_free`. These functions never return null; a null session or
  a null or non-UTF-8 argument is answered with `rejected` / `invalid_argument`
  (or an empty batch).
- `okp_session_new` returns null on failure and copies a nul-terminated message
  into `error_buffer` when one is given.
- JSON never carries video frames.
