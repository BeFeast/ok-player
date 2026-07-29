#!/usr/bin/env python3
"""Capture the compositor's own composited output as a PNG.

This is deliberately not a client-side screenshot. The rows that assert compositor state
- fullscreen covering the monitor, the shell's top bar gone - are only answered by what
the compositor actually put on the screen, so the frame comes from Mutter's ScreenCast
API through PipeWire, the same path a remote-desktop session uses.

GNOME refuses `org.gnome.Shell.Screenshot` to unprivileged callers and there is no
wlr-screencopy on this compositor, so this is the honest route rather than a convenience.
"""

from __future__ import annotations

import subprocess
import sys
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

STREAM_TIMEOUT_SECONDS = 5.0


def fail(message: str) -> None:
    print(f"capture-screen: {message}", file=sys.stderr)
    raise SystemExit(3)


def main() -> int:
    if len(sys.argv) < 2:
        fail("usage: capture-screen.py OUTPUT.png [CONNECTOR]")
    output = sys.argv[1]
    connector = sys.argv[2] if len(sys.argv) > 2 else None

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    screen_cast = Gio.DBusProxy.new_sync(
        bus,
        Gio.DBusProxyFlags.NONE,
        None,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
        None,
    )
    session_path = screen_cast.call_sync(
        "CreateSession", GLib.Variant("(a{sv})", ({},)), Gio.DBusCallFlags.NONE, 5000, None
    ).unpack()[0]
    session = Gio.DBusProxy.new_sync(
        bus,
        Gio.DBusProxyFlags.NONE,
        None,
        "org.gnome.Mutter.ScreenCast",
        session_path,
        "org.gnome.Mutter.ScreenCast.Session",
        None,
    )

    if connector is None:
        display = Gio.DBusProxy.new_sync(
            bus,
            Gio.DBusProxyFlags.NONE,
            None,
            "org.gnome.Mutter.DisplayConfig",
            "/org/gnome/Mutter/DisplayConfig",
            "org.gnome.Mutter.DisplayConfig",
            None,
        )
        connector = display.call_sync(
            "GetCurrentState", None, Gio.DBusCallFlags.NONE, 5000, None
        ).unpack()[1][0][0][0]

    stream_path = session.call_sync(
        "RecordMonitor",
        GLib.Variant("(sa{sv})", (connector, {"cursor-mode": GLib.Variant("u", 0)})),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    ).unpack()[0]

    node: dict[str, int] = {}

    def on_signal(_proxy, _sender, signal, parameters):
        if signal == "PipeWireStreamAdded":
            node["id"] = parameters.unpack()[0]

    stream = Gio.DBusProxy.new_sync(
        bus,
        Gio.DBusProxyFlags.NONE,
        None,
        "org.gnome.Mutter.ScreenCast",
        stream_path,
        "org.gnome.Mutter.ScreenCast.Stream",
        None,
    )
    stream.connect("g-signal", on_signal)
    session.call_sync("Start", None, Gio.DBusCallFlags.NONE, 5000, None)

    context = GLib.MainContext.default()
    deadline = time.monotonic() + STREAM_TIMEOUT_SECONDS
    while "id" not in node and time.monotonic() < deadline:
        context.iteration(False)
        time.sleep(0.02)
    if "id" not in node:
        session.call_sync("Stop", None, Gio.DBusCallFlags.NONE, 5000, None)
        fail("the compositor never published a PipeWire node for the monitor")

    # A few buffers, because the first frame of a fresh stream can be the pre-roll one.
    pipeline = (
        f"pipewiresrc path={node['id']} num-buffers=12 do-timestamp=true"
        " ! videoconvert ! pngenc snapshot=true"
        f" ! filesink location={output}"
    )
    result = subprocess.run(
        ["gst-launch-1.0", "-q", *pipeline.split()], capture_output=True, text=True
    )
    session.call_sync("Stop", None, Gio.DBusCallFlags.NONE, 5000, None)
    if result.returncode != 0:
        fail(f"gstreamer could not encode the frame: {result.stderr.strip()[-400:]}")
    print(f"captured {output} connector={connector}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
