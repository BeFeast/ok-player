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
```

Rendering is not part of this surface yet; it is added under session names
later. The older `okp_live_session_*` functions are still exported unchanged
until the macOS shell has moved onto this ABI.

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
| Command (`okp_session_dispatch`) | `{"schema_version":1,"command":{...}}`: `open`, `close`, `set_paused`, `toggle_pause`, `seek`, `set_volume`, `set_mute`, `set_speed`, `select_track` (`id: null` turns the kind off), `set_subtitle_delay` |
| Reply | `{"schema_version":1,"result":{...}}`: `accepted {request_id}`, `no_op {}`, `rejected {reason, message}`, or `failed {request_id, message}` when the engine refused an accepted command at once |
| Query (`okp_session_query`) | `{"schema_version":1,"query":{"snapshot":{}}}`, answered with `result.snapshot.snapshot` (the full state) or `result.rejected` |
| Event batch (`okp_session_drain_events`) | `{"schema_version":1,"events":[...]}` |

An open source is `{"kind":"local"|"url","location":"..."}`. A URL is handed
to libmpv as it is; nothing resolves it yet.

The snapshot carries `status`, `source`, `time_pos`, `duration`, `volume`,
`mute`, `speed`, `subtitle_delay`, `tracks`, `chapters`, `end_reason`, `title`,
`video` (`width`, `height`, `display_aspect`), `buffering` and `cache_ahead`.

The golden documents in `rust/crates/okp-session/fixtures/` pin every shape and
every enum value. Change them only together with the protocol.

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
