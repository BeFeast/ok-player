#!/usr/bin/env python3
"""Attribute FileChooser portal calls on the session bus to the process that made them.

Counting portal traffic is not enough on a real logged-in desktop: any other application
opening a file makes the same call, so a row that merely counts `OpenFile` can report PASS
while the player asked for nothing. That is a gate green because of something other than
the behaviour it claims to check, which is exactly the failure this harness exists to
avoid.

So every call is resolved back to a process. `dbus-monitor` records the sender's unique
connection name, and the bus itself will say which PID owns that connection
(`org.freedesktop.DBus.GetConnectionUnixProcessID`), so a call counts only when the owner
is the player under test. A connection whose owner cannot be resolved is foreign by
definition - it is not the player, whose connection is alive for as long as it is running.

    portal-calls.py --log portal.log --pid 1234 [--member OpenFile]

Prints one line of attribution and exits 0 only when the player itself made a call.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

UNRESOLVED = "unresolved"


def senders(log: pathlib.Path, member: str) -> list[str]:
    """Unique connection names that called `member` on the FileChooser portal, in order."""
    found: list[str] = []
    for line in log.read_text(errors="replace").splitlines():
        if not line.startswith("method call "):
            continue
        if "interface=org.freedesktop.portal.FileChooser" not in line:
            continue
        if f"member={member}" not in line:
            continue
        for token in line.split():
            if token.startswith("sender="):
                found.append(token[len("sender=") :])
                break
    return found


def owner_pid(bus: Gio.DBusConnection, name: str) -> int | None:
    try:
        return bus.call_sync(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetConnectionUnixProcessID",
            GLib.Variant("(s)", (name,)),
            GLib.VariantType("(u)"),
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        ).unpack()[0]
    except GLib.Error:
        # The connection is gone, so it was never the player's: that one is still open.
        return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True, help="dbus-monitor capture of the portal interface")
    parser.add_argument("--pid", type=int, required=True, help="PID of the player under test")
    parser.add_argument("--member", default="OpenFile")
    args = parser.parse_args()

    log = pathlib.Path(args.log)
    if not log.is_file():
        print(f"portal-calls: no capture at {args.log}", file=sys.stderr)
        return 3

    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    except GLib.Error as error:
        print(f"portal-calls: no session bus: {error.message}", file=sys.stderr)
        return 3

    resolved: dict[str, int | None] = {}
    mine = 0
    foreign = 0
    for sender in senders(log, args.member):
        if sender not in resolved:
            resolved[sender] = owner_pid(bus, sender)
        if resolved[sender] == args.pid:
            mine += 1
        else:
            foreign += 1

    attribution = ", ".join(
        f"{name}->{pid if pid is not None else UNRESOLVED}" for name, pid in resolved.items()
    )
    print(
        f"player-calls={mine} foreign-calls={foreign} player-pid={args.pid} "
        f"senders=[{attribution or 'none'}]"
    )
    return 0 if mine else 1


if __name__ == "__main__":
    sys.exit(main())
