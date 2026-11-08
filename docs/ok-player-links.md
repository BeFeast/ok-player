# `ok-player://` links

> **Status.** This is the v1 contract that ships with #849, the change that makes the
> Linux player execute these links. A build without it recognises `ok-player://` links
> but answers every one with a "not available yet" notice. v1 is **Linux only** (the GTK
> player). Windows and macOS follow later against the same contract.

A page you write yourself, such as a local `file://` HTML page or a web app on your own
network, can start OK Player with an ordinary link:

```html
<a href="ok-player://open?url=https%3A%2F%2Fmedia.example%2Fclips%2Fday1.mp4&amp;t=90">Play from 1:30</a>
```

Clicking it asks the browser to hand the link to OK Player. The player parses the link,
checks it against the rules below and then plays, queues or rejects it. The page gets no
answer back.

PRD §13.4 owns the product decision; this file is the contract. The parser, the policy and
the plan live in `okp-core`; the Linux shell only executes the typed plan the core returns.

## Grammar

```
ok-player://<action>?<target>[&t=<time>]

action = open | add | next          ; ASCII, case-insensitive
target = url=<pct(absolute http(s) URL)>
       | path=<pct(absolute local path)>    ; exactly one target
t      = seconds (90, 90.5) | m:ss | h:mm:ss  ; alias: resume
```

`pct(...)` means percent-encoded the way JavaScript `encodeURIComponent` encodes.

## Parsing rules

1. Values are encoded like JS `encodeURIComponent` (UTF-8 percent-encoding).
2. The query is split on `&` only, and each pair on its first `=`. Each key may appear at
   most once. Two targets or a repeated key reject the request.
3. Values are percent-decoded exactly once, strictly: a `%` not followed by two hex digits
   rejects. `%252F` stays the literal text `%2F`.
4. `+` in a query value decodes to a space, for compatibility with `URLSearchParams`,
   Python `urlencode` and PHP. A real plus is `%2B`.
5. A non-empty fragment rejects. In practice that is an unencoded `#`.
6. Unknown keys are ignored (logged) and never forwarded. No engine option ever travels
   through a link (compare IINA advisory GHSA-w5xh-98j7-jp5q).
7. A malformed optional parameter (`t`) is dropped with a notice. A malformed target
   rejects the request.
8. The raw link is capped at 4096 bytes.
9. The authority must be exactly the action: userinfo (`user@open`) and a port
   (`open:8080`) reject.
10. POSIX: `path=` decodes to bytes, so file names that are not valid UTF-8 work (encode
    their raw bytes, for example `%FF`). `url=` must decode to UTF-8.

### Accepted alternative forms

- `ok-player://<action>/<absolute-local-path>`, with each path segment percent-encoded. In
  this form `+` is a literal plus. It carries local paths only. An empty path or exactly
  `/` is not a target, so `ok-player://open/?url=…` works. A URL nested in the path
  (`ok-player://open/https://…`) rejects.
- The opaque form `ok-player:<action>?…`.
- `url=file:///…` (empty host or `localhost`) is converted to the local path it names and
  then follows the path rules.

## What a link may point at

Everything below is checked in `okp-core` before anything executes.

- **`url=`** is `http` or `https` only. A host is required; `@` and `\` reject (the same
  rules as `validate_public_http_url` in `okp-core`). Everything else rejects: `smb`,
  `file://host/`, `rtsp`, `ftp`, nested `ok-player:`, and every mpv-internal protocol
  (`edl av lavfi memory fd fdclose hex slice null mf cdda dvd bd dvb appending ytdl`).
- **`path=`** is absolute, with exactly one leading `/`; `//host/…` rejects. It must name
  an existing regular file: a directory, a FIFO or anything under `/dev` rejects. The check
  runs off the UI thread with a timeout. The player plays and records the path exactly as
  given, not its canonical form, so History keys match other opens of the same path. The
  current working directory is never joined to a link's path.
- **Playlist files** (`.m3u .m3u8 .pls .cue .edl`) and **subtitle files** are not targets
  in v1 and reject.
- The engine receives a single `loadfile` argument. Nothing from the link becomes an option.

### Command line

The desktop entry starts the player as `ok-player %U`, so a link arrives as a command-line
argument. A command line that contains an `ok-player:` argument must contain exactly that
one positional argument. Otherwise the whole launch is rejected and no other switch on it
is honoured. `ok-player 'ok-player://open?path=%2Fmedia%2Fa.mkv'` works;
`ok-player --resume 10 'ok-player://open?…'` and `ok-player a.mkv 'ok-player://…'` do not.
Starting the player this way from a terminal delivers a link exactly as typed, which is
how to test a link that a browser would rewrite first, as the browser notes explain.

## Behaviour

| Action | Not running | Running, idle | Running, playing |
|---|---|---|---|
| `open` | Start, play target, show window | Play target, present | Save progress, replace, present |
| `add` | as `open` | as `open` | Append to Up Next, toast, **no raise** |
| `next` | as `open` | as `open` | Insert after current, toast, no raise |
| rejected | Window opens idle, toast with reason | toast | toast, playback untouched |

- `open` of a local file builds the folder-sibling Up Next exactly like opening the file
  from a file manager. `add` and `next` insert relative to it.
- `open` of the file that is already current, with `t=`, seeks instead of reloading.
- `t` on `add` or `next` is ignored with a notice in v1: queue items carry no start offset.
- Whether the window comes forward depends **only on the action**, never on whether the
  link was accepted. Otherwise a page could learn whether a path exists by watching its
  own `blur` or `visibilitychange` events. When the player does not present itself it
  still completes the desktop's startup notification.
- So in the "rejected" row the window does exactly what an accepted request with the same
  action would do in the same state. A rejected `open` always comes forward. A rejected
  `add` or `next` comes forward when the player is not running or idle, because an accepted
  one acts as `open` there. It stays behind while something plays.
- Requests run in arrival order. Whether a request opens or queues is decided when it is
  taken off the queue, once the player is ready, so a cold `open A` followed by `add B`
  yields `[A, B]`.
- History is recorded as for any other open. A private session suppresses it.
- A direct media URL (its last path segment has a known media extension) streams from the
  server and never starts a background Replay cache download. Page URLs for sites the
  player resolves through yt-dlp keep their usual behaviour.

### Rejections

A rejected link shows a fixed-text toast and writes one log line. The full path is kept out
of Copy Diagnostics. Nothing is sent back to the page: there is no response channel, and
the window comes forward or stays behind the same way for a played link and a rejected one,
so a page cannot tell them apart.

## Writing links

### JavaScript helper

```js
/**
 * Build an ok-player:// link (v1 contract).
 * action: "open" | "add" | "next"
 * target: { url: "https://…" } or { path: "/absolute/local/path" }
 * t:      optional start time: 90, 90.5, "1:30" or "1:02:03"
 */
function okPlayerLink(action, target, t) {
  let query;
  if (target.url !== undefined) {
    // new URL() resolves a relative link against the page and escapes characters that
    // are not legal in a URL, so the player always receives an absolute, valid URL.
    const absolute = new URL(target.url, document.baseURI).href;
    query = "url=" + encodeURIComponent(absolute);
  } else {
    query = "path=" + encodeURIComponent(target.path);
  }
  if (t !== undefined && t !== null && t !== "") {
    query += "&t=" + encodeURIComponent(String(t));
  }
  return `ok-player://${action}?${query}`;
}

// link.href = okPlayerLink("open", { url: clip.href }, 90);
```

Outside a browser (Node), drop `document.baseURI` and pass an absolute URL. On a `file://`
page a relative link resolves to a `file:///…` URL, which the player converts to a path.

The inner URL keeps its own encoding. A space in a file name is `%20` in the URL and becomes
`%2520` in the link; the player decodes once and requests `%20` from the server.

### Plain HTML

In an HTML attribute write the `&` between parameters as `&amp;`. Setting `href` from
script needs no escaping.

A local file with spaces, Unicode and `#` in its name,
`/home/alice/Videos/Café #3 – 東京.mp4`:

```html
<a href="ok-player://open?path=%2Fhome%2Falice%2FVideos%2FCaf%C3%A9%20%233%20%E2%80%93%20%E6%9D%B1%E4%BA%AC.mp4">Play</a>
```

The same file through the path form:

```html
<a href="ok-player://open/home/alice/Videos/Caf%C3%A9%20%233%20%E2%80%93%20%E6%9D%B1%E4%BA%AC.mp4">Play</a>
```

An https URL with its own query string,
`https://media.example/clips/day%201.mp4?sig=a%2Bb&exp=1700000000`:

```html
<a href="ok-player://open?url=https%3A%2F%2Fmedia.example%2Fclips%2Fday%25201.mp4%3Fsig%3Da%252Bb%26exp%3D1700000000">Play</a>
```

The same clip, starting at 1:30, and the same clip queued instead of opened:

```html
<a href="ok-player://open?url=https%3A%2F%2Fmedia.example%2Fclips%2Fday%25201.mp4%3Fsig%3Da%252Bb%26exp%3D1700000000&amp;t=1%3A30">Play from 1:30</a>
<a href="ok-player://add?url=https%3A%2F%2Fmedia.example%2Fclips%2Fday%25201.mp4%3Fsig%3Da%252Bb%26exp%3D1700000000">Add to Up Next</a>
<a href="ok-player://next?url=https%3A%2F%2Fmedia.example%2Fclips%2Fday%25201.mp4%3Fsig%3Da%252Bb%26exp%3D1700000000">Play next</a>
```

`t=90`, `t=90.5`, `t=1:30` and `t=1%3A30` are all valid; `:` needs no encoding. `resume=`
is an alias of `t=`; a link needs only one of them.

### Server-side

A server that renders links can percent-encode raw bytes, which also covers file names
that are not valid UTF-8:

```python
import html
import os
from urllib.parse import quote

def ok_player_path_link(path, action="open", t=None):
    # quote() on bytes keeps non-UTF-8 names intact; safe="" encodes "/" as well.
    link = f"ok-player://{action}?path={quote(os.fsencode(path), safe='')}"
    if t is not None:
        link += f"&t={quote(str(t), safe='')}"
    return link

anchor = f'<a href="{html.escape(ok_player_path_link("/srv/media/a b.mkv"))}">Play</a>'
```

`urllib.parse.urlencode` and PHP `http_build_query` encode a space as `+`, which rule 4
decodes back to a space, so their output works as well.

## Browser notes

- **Chromium** (and Chromium-based browsers) asks before it opens an external app. It
  offers "Always allow" only on https, `localhost` and `file:` pages. A plain-http page on
  your network prompts on every click unless the `AutoLaunchProtocolsFromOrigins` policy
  lists it. On Linux, put a JSON file in the browser's managed policy directory
  (`/etc/chromium/policies/managed/` for Chromium, `/etc/opt/chrome/policies/managed/`
  for Google Chrome) and check `chrome://policy`:

  ```json
  {
    "AutoLaunchProtocolsFromOrigins": [
      { "protocol": "ok-player", "allowed_origins": ["http://media.example:8080"] }
    ]
  }
  ```

- Browsers allow **one external launch per user gesture**. A page cannot queue three clips
  with one click; each `add` needs its own click.
- **Firefox** asks which application should open the link and remembers the answer per
  site.
- **Write valid percent-encoding; do not count on the browser to repair it.** Before
  Chromium hands a link to the desktop it re-escapes it, and every `%` that is not
  followed by two hex digits becomes `%25`. Firefox leaves every `%` as it is. A sloppy
  link such as `path=/media/100%.mp4` therefore reaches the player from Chromium as
  `100%25.mp4` and plays `/media/100%.mp4`, but reaches it from Firefox still malformed
  and is rejected (rule 3). `encodeURIComponent` never produces such a link.

## Why base64 is not accepted

Links carry their target percent-encoded, and only that way. A base64-encoded target is not
accepted:

- It hides the target. The browser's confirmation prompt, the player's log and anyone
  reading the page source see an opaque blob instead of the path or URL.
- It adds a second encoding with several incompatible variants (standard or URL-safe
  alphabet, with or without padding), and its standard alphabet contains `+` and `/`,
  which then need percent-encoding anyway. Every extra decoder is another place where two
  implementations can disagree about the same link.
- It gains nothing. `encodeURIComponent` exists in every browser and every server
  language, and percent-encoding already carries arbitrary bytes, so non-UTF-8 file names
  work without it.

## Registration on Linux

- The `.deb` desktop entry advertises `x-scheme-handler/ok-player` in its `MimeType=` line
  and starts `ok-player %U`; the package runs `update-desktop-database`. Check the current
  handler with `xdg-mime query default x-scheme-handler/ok-player`; it should print
  `com.befeast.okplayer.desktop`.
- Settings → Integration and Copy Diagnostics report whether an installed desktop entry
  advertises the scheme and whether OK Player is its current handler.
- The Flatpak build does not handle links in v1: a link shows a toast saying so.
- AppImage builds do not register the scheme in v1.

## Not in v1

- Windows and macOS. They will follow this contract.
- Link signing or tokens, and per-site allowlists: the desktop never tells the player which
  page a link came from.
- Remote-control verbs (`pause`, `seek`, `stop`). MPRIS covers that.
- A directory or several targets in one link.
- Track preselection. A `sub=` or `audio=` key is ignored like any unknown key; the command
  line ([library-integration.md](library-integration.md#inbound-launch-with-resume)) keeps
  that feature. Engine options never travel through links.

## Test page

[`ok-player-links-test.html`](ok-player-links-test.html) is a static page for live
acceptance. Open it from disk (`file://`) in the browser under test. It lists valid links
(local paths with spaces, Unicode and `#`, an http URL with its own query string, `t=`,
`add`, `next`) and hostile ones (`edl://`, `//host/x`, a UNC path, `.m3u`, malformed `%`,
an unencoded `#`, userinfo), each with the expected result, and explains how to create the
test clips.

- Queued clips live in a `queue/` subfolder, so they are never folder siblings of the
  playing clip and each `add` or `next` visibly changes Up Next.
- Each hostile link whose rule could be skipped points at a solid red clip under `trap/`
  that would play if the rule were missing, so a missing rule shows up as a red picture
  rather than a rejection for a missing file.
- The malformed-`%` rows show a terminal command, because Chromium repairs those links
  before the player sees them.
- Rejected `add` and `next` are tested while playing, idle and not running.
