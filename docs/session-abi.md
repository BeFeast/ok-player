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
| Config (`okp_session_new`) | `schema_version`, `app` (`name`, `platform`), absolute `data_dir`, `config_dir`, `cache_dir`, optional `hwdec`, ordered `mpv_options` (`name`, `value`) applied after the session defaults |
| Command (`okp_session_dispatch`) | `{"schema_version":1,"command":{...}}`: `open`, `close`, `set_paused`, `toggle_pause`, `seek`, `set_volume`, `set_mute`, `set_speed`, `select_track` (`id: null` turns the kind off), `set_subtitle_delay`, `nudge_volume` (see Transport controls) |
| Reply | `{"schema_version":1,"result":{...}}`: `accepted {request_id}`, `no_op {}`, `rejected {reason, message}`, or `failed {request_id, message}` when the engine refused an accepted command at once |
| Query (`okp_session_query`) | `{"schema_version":1,"query":{"snapshot":{}}}`, answered with `result.snapshot.snapshot` (the full state) or `result.rejected`; `{"transport":{"at":null}}`, answered with `result.transport.transport` (see Transport controls) |
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

The golden documents in `rust/crates/okp-session/fixtures/` pin every shape and
every enum value. Change them only together with the protocol.

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

The documents are pinned by `rust/crates/okp-session/fixtures/transport/`.

## Events

A batch holds at most one `snapshot_changed` event, first. Its `changes` list
the latest value of every snapshot field that changed since the previous batch,
in snapshot order, each as `{"<field>":{"value":...}}`. A field that changed
several times between drains appears once. Applying the changes in order to the
snapshot from a query gives the session's state as of the drain.

`error` events (`kind`, `message`, `request_id` when the error answers a
command) follow in the order they happened, and `shutdown` comes last.

`time_pos` and `cache_ahead` change on every frame. On their own they are
published at most about ten times a second; whenever another field changes they
are published with it at once.

## Threading and ownership

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
