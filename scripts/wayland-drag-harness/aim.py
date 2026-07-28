#!/usr/bin/env python3
"""Aim real pointer input at the player's video plane on Wayland.

The app publishes its own geometry under `OKP_DEBUG_INTERACTIONS=1` (see
`okp_core::interaction_geometry`), which is the only way a harness can learn where the
window is: no Wayland client may query another window's position, and the shells refuse
window introspection to unprivileged callers.

The record carries window-local rectangles always, and global ones when the toplevel is
fullscreen (its origin is then its monitor's origin). A windowed toplevel has no knowable
origin, so this resolves it the way the record is designed for: inject at known global
points, read back the window-local coordinates the app reports for them, and fit the
injector's own translation (and scale, since a pointer tool need not move 1:1).

    aim.py --log run.log show
    aim.py --log run.log target          # global aim point, calibrating if needed
    aim.py --log run.log click           # aim and press once

Requires ydotoold (`YDOTOOL_SOCKET`, default /tmp/.ydotool.sock). Pointer acceleration
must be off, or the fitted transform will not hold across the desktop.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time

PREFIX = "interaction: geometry"
UNKNOWN = "unknown"


def parse_fields(line: str) -> dict[str, str]:
    return dict(token.split("=", 1) for token in line.split() if "=" in token)


def number(fields: dict[str, str], key: str) -> float | None:
    value = fields.get(key)
    if value is None or value == UNKNOWN:
        return None
    try:
        return float(value)
    except ValueError:
        return None


def read_log(path: str) -> tuple[dict[str, dict[str, str]], list[dict[str, str]]]:
    """Newest complete geometry record, and every pointer sample in order."""
    record: dict[str, dict[str, str]] = {}
    pointers: list[dict[str, str]] = []
    seq: str | None = None
    with open(path, "r", errors="replace") as handle:
        for line in handle:
            if not line.startswith(PREFIX):
                continue
            fields = parse_fields(line)
            part = fields.get("part")
            if part == "pointer":
                pointers.append(fields)
                continue
            if fields.get("seq") != seq:
                seq = fields.get("seq")
                record = {}
            if part:
                record[part] = fields
    return record, pointers


def ydotool(*args: str) -> None:
    env = dict(os.environ)
    env.setdefault("YDOTOOL_SOCKET", "/tmp/.ydotool.sock")
    subprocess.run(["ydotool", *args], check=True, env=env, stdout=subprocess.DEVNULL)


def move(x: float, y: float) -> None:
    ydotool("mousemove", "--absolute", "-x", str(int(round(x))), "-y", str(int(round(y))))


def probe(path: str, point: tuple[float, float], settle: float) -> tuple[float, float] | None:
    """Move to a global point and read back where the app says the pointer landed."""
    _, before = read_log(path)
    move(*point)
    time.sleep(settle)
    _, after = read_log(path)
    fresh = after[len(before) :]
    if not fresh:
        return None
    sample = fresh[-1]
    local_x = number(sample, "local-x")
    local_y = number(sample, "local-y")
    if local_x is None or local_y is None:
        return None
    return (local_x, local_y)


class Transform:
    """global -> window-local, per axis, as local = scale * global + offset."""

    def __init__(self, scale_x: float, offset_x: float, scale_y: float, offset_y: float):
        self.scale_x = scale_x
        self.offset_x = offset_x
        self.scale_y = scale_y
        self.offset_y = offset_y

    def to_global(self, local_x: float, local_y: float) -> tuple[float, float]:
        return (
            (local_x - self.offset_x) / self.scale_x,
            (local_y - self.offset_y) / self.scale_y,
        )

    def __str__(self) -> str:
        return (
            f"local = ({self.scale_x:.3f}*x {self.offset_x:+.1f}, "
            f"{self.scale_y:.3f}*y {self.offset_y:+.1f})"
        )


def calibrate(path: str, settle: float, verbose: bool) -> Transform:
    record, _ = read_log(path)
    window = record.get("window")
    if not window:
        raise SystemExit("no geometry record: is OKP_DEBUG_INTERACTIONS=1 set?")

    origin_x = number(window, "x")
    origin_y = number(window, "y")
    if origin_x is not None and origin_y is not None:
        # The app resolved its own origin (fullscreen): no injection needed.
        return Transform(1.0, -origin_x, 1.0, -origin_y)

    width = number(window, "w") or 640.0
    height = number(window, "h") or 480.0
    desktop_x = number(window, "desktop-x") or 0.0
    desktop_y = number(window, "desktop-y") or 0.0
    desktop_w = number(window, "desktop-w") or 1920.0
    desktop_h = number(window, "desktop-h") or 1080.0

    step_x = max(int(width * 0.5), 40)
    step_y = max(int(height * 0.5), 40)
    grid = [
        (float(px), float(py))
        for py in range(int(desktop_y) + step_y // 2, int(desktop_y + desktop_h), step_y)
        for px in range(int(desktop_x) + step_x // 2, int(desktop_x + desktop_w), step_x)
    ]

    samples: list[tuple[tuple[float, float], tuple[float, float]]] = []
    for point in grid:
        landed = probe(path, point, settle)
        if landed is None:
            continue
        if verbose:
            print(f"probe global={point} -> local={landed}")
        samples.append((point, landed))
        if len(samples) >= 2:
            (g1, l1), (g2, l2) = samples[-2], samples[-1]
            if abs(g2[0] - g1[0]) > 1.0 and abs(g2[1] - g1[1]) > 1.0:
                scale_x = (l2[0] - l1[0]) / (g2[0] - g1[0])
                scale_y = (l2[1] - l1[1]) / (g2[1] - g1[1])
                if scale_x > 0.05 and scale_y > 0.05:
                    return Transform(
                        scale_x,
                        l1[0] - scale_x * g1[0],
                        scale_y,
                        l1[1] - scale_y * g1[1],
                    )
    if samples:
        (g1, l1) = samples[-1]
        return Transform(1.0, l1[0] - g1[0], 1.0, l1[1] - g1[1])
    raise SystemExit("no pointer sample: injected motion never reached the window")


def aim_point(path: str, part: str, settle: float, verbose: bool) -> tuple[float, float]:
    record, _ = read_log(path)
    plane = record.get(part)
    if plane is None:
        raise SystemExit(f"no {part} plane in the geometry record")
    center_x = number(plane, "center-x")
    center_y = number(plane, "center-y")
    if center_x is not None and center_y is not None:
        return (center_x, center_y)

    transform = calibrate(path, settle, verbose)
    if verbose:
        print(f"transform {transform}")
    local_x = (number(plane, "local-x") or 0.0) + (number(plane, "w") or 0.0) / 2.0
    local_y = (number(plane, "local-y") or 0.0) + (number(plane, "h") or 0.0) / 2.0
    target = transform.to_global(local_x, local_y)

    # Confirm delivery before pressing: move there and check the app reports the point we
    # meant to hit. This is motion only - the press that follows is still the first press.
    landed = probe(path, target, settle)
    if landed is not None:
        error = (abs(landed[0] - local_x), abs(landed[1] - local_y))
        print(
            f"delivery check: wanted local=({local_x:.1f},{local_y:.1f}) "
            f"got local=({landed[0]:.1f},{landed[1]:.1f}) error=({error[0]:.1f},{error[1]:.1f})"
        )
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True, help="app stderr log with the diagnostics")
    parser.add_argument("--part", default="drag-target", help="plane to aim at")
    parser.add_argument("--settle", type=float, default=0.35, help="seconds after each move")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("action", choices=["show", "target", "click"])
    args = parser.parse_args()

    if args.action == "show":
        record, pointers = read_log(args.log)
        for part, fields in record.items():
            print(part, " ".join(f"{key}={value}" for key, value in fields.items()))
        if pointers:
            print("pointer", " ".join(f"{key}={value}" for key, value in pointers[-1].items()))
        return 0

    x, y = aim_point(args.log, args.part, args.settle, args.verbose)
    print(f"target {args.part} global=({x:.1f},{y:.1f})")
    if args.action == "click":
        move(x, y)
        time.sleep(args.settle)
        ydotool("click", "0xC0")
    return 0


if __name__ == "__main__":
    sys.exit(main())
