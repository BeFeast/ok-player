#!/usr/bin/env python3
"""Write the Continue-watching fixture the shelf-layout smoke renders (#702).

The point of the fixture is the content the shelf used to be sized by, so the four rows
are chosen to be hostile: a very long title carrying emoji and hashtags, a short title, a
deep path with a long file name, and one item deliberately left without a poster. A fourth
row sits behind the welcome item limit so the shelf is genuinely truncating.

Everything lands under one directory: `media/` holds the (empty) files history rows must
point at to be listable, `posters/` holds poster frames keyed by file stem for
`OKP_POSTER_FIXTURE_DIR`, and `state/ok-player/history.json` is the history document to
point `XDG_STATE_HOME` at.
"""

import json
import os
import subprocess
import sys
import time

LONG_TITLE = (
    "\U0001f3ac The Absolutely Enormous Director's Cut of a Film Whose Title Refuses "
    "to End \U0001f37f #restored #4k #fanedit #directorscut — Part One"
)

LONG_RELATIVE_PATH = os.path.join(
    "Documentaries",
    "Season 04 - Remastered Edition",
    "Extras and behind the scenes footage",
    "Disc 2 of 3 - director commentary track",
    "a-very-long-file-name-that-nobody-would-ever-type-by-hand-episode-11.mkv",
)


def write_poster(path):
    subprocess.run(
        [
            "ffmpeg", "-nostdin", "-y", "-loglevel", "error",
            "-f", "lavfi", "-i", "testsrc=size=320x180:rate=1:duration=1",
            "-frames:v", "1", path,
        ],
        check=True,
    )


def main():
    if len(sys.argv) != 2:
        print("usage: make-linux-recents-fixture.py <directory>", file=sys.stderr)
        return 2

    root = sys.argv[1]
    media = os.path.join(root, "media")
    posters = os.path.join(root, "posters")
    state = os.path.join(root, "state", "ok-player")
    for directory in (media, posters, state):
        os.makedirs(directory, exist_ok=True)

    now = int(time.time())
    # (relative path, title, poster wanted, seconds since it was last opened)
    rows = [
        ("long-title.mkv", LONG_TITLE, True, 10),
        ("short.mkv", "Short", True, 20),
        (LONG_RELATIVE_PATH, None, False, 30),
        ("overflow.mkv", "Overflow item that never reaches the shelf", True, 900),
    ]

    files = {}
    for relative, title, wants_poster, age in rows:
        path = os.path.join(media, relative)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        # An empty file is enough: the shell lists a history row when its path is a file,
        # and an empty one also guarantees no poster can be derived from it.
        open(path, "wb").close()
        entry = {
            "position": 900.0,
            "duration": 5400.0,
            "finished": False,
            "updated_at_unix": now - age,
        }
        if title is not None:
            entry["title"] = title
        files[path] = entry
        if wants_poster:
            stem = os.path.splitext(os.path.basename(path))[0]
            write_poster(os.path.join(posters, f"{stem}.jpg"))

    with open(os.path.join(state, "history.json"), "w", encoding="utf-8") as handle:
        json.dump({"version": 2, "files": files}, handle, indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main())
