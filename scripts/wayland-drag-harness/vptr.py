#!/usr/bin/env python3
"""Persistent wlroots virtual pointer, driven by stdin commands.

Commands (one per line):
  abs X Y            absolute move (extent 1280x720)
  btn press|release  left button
  sleep MS
  quit
"""
import sys, time, os

from pywayland.scanner import Protocol
from pywayland.client import Display

# Generate python bindings for the wlr protocol + core at runtime
import tempfile, importlib

HERE = os.path.dirname(os.path.abspath(__file__))
XML = os.path.join(HERE, "wlr-virtual-pointer-unstable-v1.xml")

gen_root = tempfile.mkdtemp(prefix="vptr-proto-")
gen_dir = os.path.join(gen_root, "protocols")
os.makedirs(gen_dir)
open(os.path.join(gen_dir, "__init__.py"), "w").close()
core_xml = "/usr/share/wayland/wayland.xml"
protos = [Protocol.parse_file(core_xml), Protocol.parse_file(XML)]
imports = {}
for p in protos:
    for iface in p.interface:
        imports[iface.name] = p.name
for p in protos:
    p.output(gen_dir, imports)
sys.path.insert(0, gen_root)

wlr = importlib.import_module("protocols.wlr_virtual_pointer_unstable_v1")

EXTENT_W, EXTENT_H = 1280, 720
BTN_LEFT = 0x110

display = Display()
display.connect()

manager = None
seat = None

def handle_global(registry, id_, interface, version):
    global manager, seat
    if interface == "zwlr_virtual_pointer_manager_v1":
        manager = registry.bind(id_, wlr.ZwlrVirtualPointerManagerV1, min(version, 2))
    elif interface == "wl_seat" and seat is None:
        from protocols.wayland import WlSeat
        seat = registry.bind(id_, WlSeat, min(version, 5))

registry = display.get_registry()
registry.dispatcher["global"] = handle_global
display.roundtrip()

if manager is None:
    print("NO zwlr_virtual_pointer_manager_v1", flush=True)
    sys.exit(2)

ptr = manager.create_virtual_pointer(seat)
display.roundtrip()
print("VPTR READY", flush=True)

def now_ms():
    return int(time.monotonic() * 1000) & 0xFFFFFFFF

for line in sys.stdin:
    parts = line.strip().split()
    if not parts:
        continue
    cmd = parts[0]
    if cmd == "abs":
        x, y = int(parts[1]), int(parts[2])
        ptr.motion_absolute(now_ms(), x, y, EXTENT_W, EXTENT_H)
        ptr.frame()
    elif cmd == "btn":
        state = 1 if parts[1] == "press" else 0
        ptr.button(now_ms(), BTN_LEFT, state)
        ptr.frame()
    elif cmd == "sleep":
        display.flush()
        time.sleep(int(parts[1]) / 1000.0)
        continue
    elif cmd == "quit":
        break
    display.flush()
    print("ok " + line.strip(), flush=True)

ptr.destroy()
display.roundtrip()
display.disconnect()
