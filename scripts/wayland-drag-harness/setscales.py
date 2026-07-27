#!/usr/bin/env python3
"""Apply the sindri monitor layout to headless mutter: primary 3840x2160@scale2
at 0,0 plus 1920x1080@scale1.667 at logical (1920,432)."""
import os
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

ADDR = os.environ.get("OKP_BUS", "unix:path=/tmp/okp-drag-repro/bus")
DEST = "org.gnome.Mutter.DisplayConfig"
PATH = "/org/gnome/Mutter/DisplayConfig"
IFACE = "org.gnome.Mutter.DisplayConfig"

bus = Gio.DBusConnection.new_for_address_sync(
    ADDR,
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
    | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
    None, None)

state = bus.call_sync(DEST, PATH, IFACE, "GetCurrentState", None, None, 0, -1, None).unpack()
serial, monitors, logical_monitors, props = state

mons = []
for spec, modes, mprops in monitors:
    connector = spec[0]
    cur = None
    pref = None
    for m in modes:
        mode_id, w, h, rate, pscale, sscales, mp = m
        if mp.get("is-current"):
            cur = (mode_id, w, h, sscales)
        if mp.get("is-preferred"):
            pref = (mode_id, w, h, sscales)
    mons.append((connector, cur or pref))

print("monitors:", [(c, m[0], m[1], m[2], m[3]) for c, m in mons])
assert len(mons) >= 2, "need two virtual monitors"

big = next((c, m) for c, m in mons if m[1] == 3840)
small = next((c, m) for c, m in mons if m[1] == 1920)

def pick_scale(supported, want):
    return min(supported, key=lambda s: abs(s - want))

s_big = pick_scale(big[1][3], 2.0)
s_small = pick_scale(small[1][3], 1.6666666269302368)
print("scales chosen:", s_big, s_small)

logical = [
    (0, 0, s_big, 0, True, [(big[0], big[1][0], {})]),
    (1920, 432, s_small, 0, False, [(small[0], small[1][0], {})]),
]

params = GLib.Variant("(uua(iiduba(ssa{sv}))a{sv})", (
    serial, 1, logical, {"layout-mode": GLib.Variant("u", 1)},
))
bus.call_sync(DEST, PATH, IFACE, "ApplyMonitorsConfig", params, None, 0, -1, None)
print("APPLIED")
