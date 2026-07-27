#!/usr/bin/env python3
"""Pointer injection for headless mutter via org.gnome.Mutter.RemoteDesktop.

Same stdin protocol as vptr.py: abs X Y | btn press|release | sleep MS | quit.
'abs' is emulated: park at (0,0) with a huge relative move, then move out.
"""
import sys, time, os
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

ADDR = os.environ.get("OKP_BUS", "unix:path=/tmp/okp-drag-repro/bus")
DEST = "org.gnome.Mutter.RemoteDesktop"
BTN_LEFT = 0x110

bus = Gio.DBusConnection.new_for_address_sync(
    ADDR,
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
    | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
    None, None)

res = bus.call_sync(DEST, "/org/gnome/Mutter/RemoteDesktop",
                    "org.gnome.Mutter.RemoteDesktop", "CreateSession",
                    None, GLib.VariantType("(o)"), 0, -1, None)
session = res.unpack()[0]

def scall(method, params=None, reply=None):
    bus.call_sync(DEST, session, "org.gnome.Mutter.RemoteDesktop.Session",
                  method, params, reply, 0, -1, None)

scall("Start")
print("MPTR READY session=" + session, flush=True)

cur = [None, None]

def rel(dx, dy):
    scall("NotifyPointerMotionRelative", GLib.Variant("(dd)", (float(dx), float(dy))))

def go_abs(x, y):
    rel(-10000, -10000)          # clamp to top-left corner
    rel(x, y)
    cur[0], cur[1] = x, y

for line in sys.stdin:
    parts = line.strip().split()
    if not parts:
        continue
    cmd = parts[0]
    try:
        if cmd == "abs":
            x, y = int(parts[1]), int(parts[2])
            if cur[0] is not None and abs(x - cur[0]) + abs(y - cur[1]) < 200:
                rel(x - cur[0], y - cur[1])   # small step: genuine relative motion
                cur[0], cur[1] = x, y
            else:
                go_abs(x, y)
        elif cmd == "btn":
            pressed = parts[1] == "press"
            scall("NotifyPointerButton", GLib.Variant("(ib)", (BTN_LEFT, pressed)))
        elif cmd == "key":
            keysym = int(parts[1], 0)
            scall("NotifyKeyboardKeysym", GLib.Variant("(ub)", (keysym, True)))
            scall("NotifyKeyboardKeysym", GLib.Variant("(ub)", (keysym, False)))
        elif cmd == "sleep":
            time.sleep(int(parts[1]) / 1000.0)
            continue
        elif cmd == "quit":
            break
        print("ok " + line.strip(), flush=True)
    except Exception as e:
        # A rejected notification means the round is not being delivered as
        # scripted; continuing would let the driver report a false "survived".
        print("ERR " + line.strip() + " -> " + str(e), flush=True)
        sys.exit(1)

try:
    scall("Stop")
except Exception:
    pass
