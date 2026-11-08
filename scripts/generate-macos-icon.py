#!/usr/bin/env python3
"""Generate the macOS app icon (.icns) from the app icon SVGs the Linux packages ship.

Usage: generate-macos-icon.py OUTPUT_ICNS

The tile goes on Apple's macOS icon grid: an 824-unit body centred on a 1024-unit
canvas, so the icon sits at the same visual size as other apps in the Dock and
Finder. Canvases of 64 pixels and below take the size-tuned SVGs of the hicolor
theme (branding.rs gives them a heavier mark, and only the play triangle at 16 px);
larger canvases take the master SVG.

Rasterising needs rsvg-convert (librsvg). The .icns is checked in at
macos/Resources/AppIcon.icns, so the macOS build does not need librsvg: rerun this
script after the icon source changes.
"""
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
LINUX_ICONS = ROOT / "rust" / "packaging" / "linux"
MASTER_SVG = LINUX_ICONS / "com.befeast.okplayer.svg"
SIZE_TUNED_SVG = {
    size: LINUX_ICONS / "icons" / "hicolor" / f"{size}x{size}" / "apps" / "com.befeast.okplayer.svg"
    for size in (16, 32, 64)
}
SVG_NS = "http://www.w3.org/2000/svg"
CANVAS, BODY = 1024, 824

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


def on_macos_grid(source_svg, pixels):
    """SVG text that draws `source_svg` as the body of a macOS icon of `pixels` px."""
    ET.register_namespace("", SVG_NS)
    source = ET.parse(source_svg).getroot()
    view_box = source.get("viewBox")
    if not view_box:
        raise ValueError(f"{source_svg} has no viewBox")
    canvas = ET.Element(f"{{{SVG_NS}}}svg", {
        "width": str(pixels), "height": str(pixels), "viewBox": f"0 0 {CANVAS} {CANVAS}",
    })
    inset = str((CANVAS - BODY) // 2)
    body = ET.SubElement(canvas, f"{{{SVG_NS}}}svg", {
        "x": inset, "y": inset, "width": str(BODY), "height": str(BODY), "viewBox": view_box,
    })
    body.extend(list(source))
    return ET.tostring(canvas, encoding="unicode")


def rasterise(svg_text, pixels, scratch):
    source = pathlib.Path(scratch, f"icon-{pixels}.svg")
    source.write_text(svg_text, encoding="utf-8")
    output = pathlib.Path(scratch, f"icon-{pixels}.png")
    subprocess.run(["rsvg-convert", "-w", str(pixels), "-h", str(pixels), "-f", "png",
                    "-o", str(output), str(source)], check=True)
    return output.read_bytes()


def main(argv):
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    if not shutil.which("rsvg-convert"):
        print("generate-macos-icon: rsvg-convert (librsvg) is required", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory() as scratch:
        images = {}
        for _, pixels in ICNS_ELEMENTS:
            if pixels not in images:
                svg = SIZE_TUNED_SVG.get(pixels, MASTER_SVG)
                images[pixels] = rasterise(on_macos_grid(svg, pixels), pixels, scratch)
        data = pack_icns([(kind, images[pixels]) for kind, pixels in ICNS_ELEMENTS])
    pathlib.Path(argv[1]).write_bytes(data)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
