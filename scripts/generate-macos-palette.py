#!/usr/bin/env python3
"""Generate the macOS shell's colour table from brand/palette.json.

The build runs this before compiling the AppKit shell, so the Swift source never
carries its own copy of the palette. Every semantic role becomes a case of
`PaletteRole` with its light and dark sRGB value; the shell turns each case into
a dynamic NSColor that follows the system appearance.

Usage: generate-macos-palette.py PALETTE_JSON OUTPUT_SWIFT
"""
import json
import re
import sys

HEX = re.compile(r"^#([0-9A-Fa-f]{6})$")
ROLE = re.compile(r"^[a-z][A-Za-z0-9]*$")
APPEARANCES = ("light", "dark")


def load_roles(palette):
    """Return [(role, light_hex, dark_hex)] in the palette's own role order."""
    tables = {}
    for appearance in APPEARANCES:
        table = palette.get(appearance)
        if not isinstance(table, dict) or not table:
            raise ValueError(f"palette has no '{appearance}' role table")
        tables[appearance] = table
    light, dark = tables["light"], tables["dark"]
    if set(light) != set(dark):
        missing = sorted(set(light) ^ set(dark))
        raise ValueError(f"light and dark define different roles: {', '.join(missing)}")
    roles = []
    for role in light:
        if not ROLE.match(role):
            raise ValueError(f"role '{role}' is not a lower-camel-case identifier")
        values = []
        for appearance in APPEARANCES:
            value = tables[appearance][role]
            match = HEX.match(value) if isinstance(value, str) else None
            if not match:
                raise ValueError(f"{appearance}.{role} = {value!r} is not #RRGGBB")
            values.append(match.group(1).upper())
        roles.append((role, values[0], values[1]))
    return roles


def swift_source(palette):
    roles = load_roles(palette)
    name = palette.get("name", "")
    lines = [
        "// Generated from brand/palette.json by scripts/generate-macos-palette.py",
        "// on every build. Change the JSON, not this file.",
        "",
        f"/// Semantic colour roles of the {name} palette as sRGB 0xRRGGBB values.",
        "enum PaletteRole: CaseIterable {",
    ]
    lines += [f"    case {role}" for role, _, _ in roles]
    lines += [
        "",
        "    var srgb: (light: UInt32, dark: UInt32) {",
        "        switch self {",
    ]
    lines += [
        f"        case .{role}: return (light: 0x{light}, dark: 0x{dark})"
        for role, light, dark in roles
    ]
    lines += ["        }", "    }", "}", ""]
    return "\n".join(lines)


def main(argv):
    if len(argv) != 3:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    try:
        with open(argv[1], encoding="utf-8") as handle:
            source = swift_source(json.load(handle))
    except (OSError, ValueError) as error:
        print(f"generate-macos-palette: {error}", file=sys.stderr)
        return 1
    with open(argv[2], "w", encoding="utf-8") as handle:
        handle.write(source)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
