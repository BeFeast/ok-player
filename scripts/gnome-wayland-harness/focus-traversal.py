#!/usr/bin/env python3
"""Decide whether Tab actually moved keyboard focus.

The row asserts traversal, not keystrokes. A harness that only counted the keys it sent
would pass while focus sat still, so this reads the stops the shell reported and requires
three separate things: enough stops to be a traversal, enough *distinct* widgets that
focus is not oscillating between two, and a return path proving Shift+Tab walks back into
ground Tab already covered.

    focus-traversal.py --log app.log --from-line N --forward 6 --backward 3

Prints one line of reasoning and exits 0 only when the traversal is real.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

PREFIX = "interaction: focus target="


def stops(log: pathlib.Path, skip: int) -> list[str]:
    found = []
    for line in log.read_text(errors="replace").splitlines():
        marker = line.find(PREFIX)
        if marker == -1:
            continue
        found.append(line[marker + len(PREFIX) :].split()[0])
    return found[skip:]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True)
    parser.add_argument("--from-line", type=int, default=0, help="stops already present before the gesture")
    parser.add_argument("--forward", type=int, required=True, help="Tab presses sent")
    parser.add_argument("--backward", type=int, required=True, help="Shift+Tab presses sent")
    args = parser.parse_args()

    observed = stops(pathlib.Path(args.log), args.from_line)
    forward = observed[: args.forward]
    backward = observed[args.forward :]
    distinct_forward = len(set(forward))
    returned = sum(1 for stop in backward if stop in set(forward))

    # Half the presses is the floor: GTK legitimately skips containers and wraps, but a
    # focus chain that answers fewer than that is not traversing.
    enough_stops = len(observed) >= (args.forward + args.backward) // 2
    enough_widgets = distinct_forward >= max(2, args.forward // 2)
    walks_back = args.backward == 0 or returned >= 1

    reason = (
        f"stops={len(observed)} forward={len(forward)} distinct-forward={distinct_forward} "
        f"returned={returned} chain={'>'.join(observed) or 'none'}"
    )
    if enough_stops and enough_widgets and walks_back:
        print(f"pass {reason}")
        return 0
    print(f"fail {reason}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
