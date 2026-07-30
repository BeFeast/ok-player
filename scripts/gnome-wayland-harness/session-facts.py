#!/usr/bin/env python3
"""Report the session an automated live row would be attested against.

Everything here answers a question the evidence has to carry: which compositor decided
the outcomes, at what version, and how the desktop was laid out and scaled. Nothing here
identifies the machine - the repository is public - so no hostname, user, or path is read
or emitted.

Exits non-zero when the session is not a real Wayland session, because a live row
recorded anywhere else is not the thing the release gate is asking about.
"""

from __future__ import annotations

import json
import os
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402


def fail(message: str) -> "None":
    print(f"session-facts: {message}", file=sys.stderr)
    raise SystemExit(3)


def session_bus() -> Gio.DBusConnection:
    try:
        return Gio.bus_get_sync(Gio.BusType.SESSION, None)
    except GLib.Error as error:
        fail(f"no session bus: {error.message}")


def proxy(bus: Gio.DBusConnection, name: str, path: str, interface: str) -> Gio.DBusProxy:
    try:
        return Gio.DBusProxy.new_sync(
            bus, Gio.DBusProxyFlags.NONE, None, name, path, interface, None
        )
    except GLib.Error as error:
        fail(f"{interface} unavailable: {error.message}")


def compositor_version(bus: Gio.DBusConnection) -> str:
    shell = proxy(bus, "org.gnome.Shell", "/org/gnome/Shell", "org.freedesktop.DBus.Properties")
    try:
        return shell.call_sync(
            "Get",
            GLib.Variant("(ss)", ("org.gnome.Shell", "ShellVersion")),
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        ).unpack()[0]
    except GLib.Error as error:
        fail(f"no shell version: {error.message}")


def monitors(bus: Gio.DBusConnection) -> list[dict[str, object]]:
    display = proxy(
        bus,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    try:
        state = display.call_sync("GetCurrentState", None, Gio.DBusCallFlags.NONE, 5000, None)
    except GLib.Error as error:
        fail(f"no monitor layout: {error.message}")
    _serial, physical, logical, _properties = state.unpack()

    # The logical monitors carry the scale the compositor actually applies; the physical
    # ones carry the mode. A row's evidence needs both, keyed by connector.
    modes: dict[str, tuple[int, int]] = {}
    for (connector, _vendor, _product, _serial_number), mode_list, _props in physical:
        for name, width, height, _refresh, _preferred_scale, _scales, flags in mode_list:
            if flags.get("is-current"):
                modes[connector] = (width, height)
                break
        modes.setdefault(connector, (0, 0))

    resolved: list[dict[str, object]] = []
    for _x, _y, scale, _transform, _primary, assigned, _props in logical:
        for connector, _vendor, _product, _serial_number in assigned:
            width, height = modes.get(connector, (0, 0))
            resolved.append(
                {
                    "connector": connector,
                    "width": width,
                    "height": height,
                    "scale": round(float(scale), 6),
                }
            )
    if not resolved:
        fail("the compositor reports no logical monitor")
    return resolved


def main() -> int:
    session_type = os.environ.get("XDG_SESSION_TYPE", "")
    if not session_type:
        # Set for a login shell, absent under a CI runner service; fall back to the fact
        # that reaches this process either way.
        session_type = "wayland" if os.environ.get("WAYLAND_DISPLAY") else "unknown"
    if session_type != "wayland":
        fail(f"not a wayland session (XDG_SESSION_TYPE={session_type!r})")
    if not os.environ.get("WAYLAND_DISPLAY"):
        fail("WAYLAND_DISPLAY is unset: there is no session to drive")

    bus = session_bus()
    facts = {
        "session_type": "wayland",
        "compositor": "gnome-shell",
        "compositor_version": compositor_version(bus),
        "monitors": monitors(bus),
        "input_injector": os.environ.get("OKP_HARNESS_INJECTOR", "uinput"),
    }
    json.dump(facts, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
