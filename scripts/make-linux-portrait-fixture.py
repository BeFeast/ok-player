#!/usr/bin/env python3
"""Write the portrait/landscape media the idle-geometry smoke plays (#716).

The operator's case is a 9:16 clip: fitting the window to it produces a window far
narrower than the idle canvas was ever laid out for, and leaving playback used to leave
the idle surface inside that shape. The landscape clip is the second half of the
no-drift check - a portrait file followed by a landscape file must still return to the
one geometry the user had before either of them opened.

Both clips are long enough that a loaded builder cannot run out of media while the smoke
is still measuring, and cheap enough to generate that the smoke does not need a checked-in
binary.
"""

import os
import subprocess
import sys

CLIPS = {
    # name: (width, height)
    "portrait.mkv": (1080, 1920),
    "landscape.mkv": (1920, 1080),
}
DURATION = 600


def write_clip(path, width, height):
    subprocess.run(
        [
            "ffmpeg", "-nostdin", "-y", "-loglevel", "error",
            "-f", "lavfi",
            "-i", f"testsrc=size={width}x{height}:rate=5:duration={DURATION}",
            "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
            "-g", "10", "-an", path,
        ],
        check=True,
    )


def main():
    if len(sys.argv) != 2:
        print("usage: make-linux-portrait-fixture.py <directory>", file=sys.stderr)
        return 2

    media = os.path.join(sys.argv[1], "media")
    os.makedirs(media, exist_ok=True)
    for name, (width, height) in CLIPS.items():
        write_clip(os.path.join(media, name), width, height)
    return 0


if __name__ == "__main__":
    sys.exit(main())
