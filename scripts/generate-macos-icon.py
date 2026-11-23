#!/usr/bin/env python3
"""Generate the macOS app icon and Welcome tile from the OK Player About mark.

Usage: generate-macos-icon.py OUTPUT_ICNS [WELCOME_PNG]

The Dock and Finder icon uses an 824-unit squircle on a 1024-unit transparent
canvas. The Welcome tile uses the same artwork without the inset, at 2x.

Rasterising needs rsvg-convert (librsvg). The generated assets are checked in so the
macOS build does not need librsvg. Regenerate them when the mark or palette changes.
"""
import json
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
BRAND = ROOT / "brand"
MARK = BRAND / "ok-player-mark.svg"
SVG_NS = "http://www.w3.org/2000/svg"
CANVAS, BODY = 1024, 824
TILE = "M33 0H95C119 0 128 9 128 33V95C128 119 119 128 95 128H33C9 128 0 119 0 95V33C0 9 9 0 33 0Z"

# (ICNS element type, pixel size): the ten images of a standard macOS iconset,
# 16/32/128/256/512 points at 1x and 2x.
ICNS_ELEMENTS = (
    ("icp4", 16), ("ic11", 32),
    ("icp5", 32), ("ic12", 64),
    ("ic07", 128), ("ic13", 256),
    ("ic08", 256), ("ic14", 512),
    ("ic09", 512), ("ic10", 1024),
)
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def pack_icns(elements):
    """Pack [(type, png_bytes)] into an ICNS container."""
    body = b""
    for kind, data in elements:
        if len(kind) != 4 or not data.startswith(PNG_SIGNATURE):
            raise ValueError(f"element {kind!r} must be a four-character type with PNG data")
        body += kind.encode("ascii") + struct.pack(">I", 8 + len(data)) + data
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def read_icns(data):
    """Return [(type, payload)] from an ICNS container; raise ValueError when malformed."""
    if data[:4] != b"icns" or struct.unpack(">I", data[4:8])[0] != len(data):
        raise ValueError("not an ICNS container of the stated length")
    elements, offset = [], 8
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValueError("truncated element header")
        kind = data[offset:offset + 4].decode("ascii")
        length = struct.unpack(">I", data[offset + 4:offset + 8])[0]
        if length < 8 or offset + length > len(data):
            raise ValueError(f"element {kind!r} overruns the container")
        elements.append((kind, data[offset + 8:offset + length]))
        offset += length
    return elements


def png_size(data):
    """(width, height) from a PNG's IHDR chunk."""
    if not data.startswith(PNG_SIGNATURE) or data[12:16] != b"IHDR":
        raise ValueError("not a PNG")
    return struct.unpack(">II", data[16:24])


def icon_svg(pixels, *, dock):
    """Compose the original mark on a pale squircle, with padding for the Dock."""
    ET.register_namespace("", SVG_NS)
    tag = f"{{{SVG_NS}}}"
    light = json.loads((BRAND / "palette.json").read_text(encoding="utf-8"))["light"]
    canvas = ET.Element(f"{tag}svg", {
        "width": str(pixels), "height": str(pixels), "viewBox": f"0 0 {CANVAS} {CANVAS}",
    })
    size = BODY if dock else CANVAS
    inset = str((CANVAS - size) // 2)
    tile = ET.SubElement(canvas, f"{tag}svg", {
        "x": inset, "y": inset, "width": str(size), "height": str(size), "viewBox": "0 0 128 128",
    })
    ET.SubElement(tile, f"{tag}path", {"d": TILE, "fill": light["canvas"]})
    # A subtle edge keeps the tile distinct against a light wallpaper.
    ET.SubElement(tile, f"{tag}path", {
        "d": TILE, "fill": "none", "stroke": light["borderSubtle"], "stroke-width": "2",
        "transform": "translate(1 1) scale(0.984375)",
    })
    mark = ET.parse(MARK).getroot()
    mark.attrib.update({"x": "8", "y": "20", "width": "112", "height": "88"})
    tile.append(mark)
    return ET.tostring(canvas, encoding="unicode")


def rasterise(svg_text, pixels, scratch):
    source = pathlib.Path(scratch, f"icon-{pixels}.svg")
    source.write_text(svg_text, encoding="utf-8")
    output = pathlib.Path(scratch, f"icon-{pixels}.png")
    subprocess.run(["rsvg-convert", "-w", str(pixels), "-h", str(pixels), "-f", "png",
                    "-o", str(output), str(source)], check=True)
    return output.read_bytes()


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    if not shutil.which("rsvg-convert"):
        print("generate-macos-icon: rsvg-convert (librsvg) is required", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory() as scratch:
        images = {}
        for _, pixels in ICNS_ELEMENTS:
            if pixels not in images:
                images[pixels] = rasterise(icon_svg(pixels, dock=True), pixels, scratch)
        if len(argv) == 3:
            pathlib.Path(argv[2]).write_bytes(rasterise(icon_svg(96, dock=False), 96, scratch))
        data = pack_icns([(kind, images[pixels]) for kind, pixels in ICNS_ELEMENTS])
    pathlib.Path(argv[1]).write_bytes(data)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
