#!/usr/bin/env python3
"""The macOS shell's colour table is generated from brand/palette.json."""
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
GENERATOR = ROOT / "scripts" / "generate-macos-palette.py"
PALETTE = ROOT / "brand" / "palette.json"
CASE = re.compile(r"^\s*case \.(\w+): return \(light: 0x([0-9A-F]{6}), dark: 0x([0-9A-F]{6})\)$")


def generate(palette_path):
    """Run the generator the way the build script does; return (exit code, Swift, stderr)."""
    with tempfile.TemporaryDirectory() as scratch:
        output = Path(scratch, "Palette.swift")
        result = subprocess.run(
            [sys.executable, str(GENERATOR), str(palette_path), str(output)],
            capture_output=True, text=True,
        )
        swift = output.read_text(encoding="utf-8") if output.exists() else ""
    return result.returncode, swift, result.stderr


def generated_table(swift):
    """Read the emitted Swift back into {role: (light, dark)} as '#RRGGBB' strings."""
    table = {}
    for line in swift.splitlines():
        match = CASE.match(line)
        if match:
            table[match.group(1)] = ("#" + match.group(2), "#" + match.group(3))
    return table


def json_table(palette):
    return {role: (palette["light"][role].upper(), palette["dark"][role].upper())
            for role in palette["light"]}


class MacosPaletteTests(unittest.TestCase):
    def generate_from(self, palette):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch, "palette.json")
            path.write_text(json.dumps(palette), encoding="utf-8")
            return generate(path)

    def test_every_role_carries_the_brand_json_values(self):
        code, swift, stderr = generate(PALETTE)
        self.assertEqual(code, 0, stderr)
        palette = json.loads(PALETTE.read_text(encoding="utf-8"))
        self.assertEqual(generated_table(swift), json_table(palette))

    def test_roles_the_shell_paints_with_follow_the_json(self):
        code, swift, stderr = generate(PALETTE)
        self.assertEqual(code, 0, stderr)
        table = generated_table(swift)
        palette = json.loads(PALETTE.read_text(encoding="utf-8"))
        for role in ("canvas", "surface", "surfaceRaised", "borderSubtle", "text", "textMuted",
                     "accent", "onAccent", "focus", "danger"):
            with self.subTest(role=role):
                self.assertEqual(table[role], (palette["light"][role], palette["dark"][role]))

    def test_a_changed_json_value_reaches_the_generated_table(self):
        palette = json.loads(PALETTE.read_text(encoding="utf-8"))
        palette["dark"]["canvas"] = "#123456"
        palette["light"]["accent"] = "#abcdef"
        code, swift, stderr = self.generate_from(palette)
        self.assertEqual(code, 0, stderr)
        table = generated_table(swift)
        self.assertEqual(table["canvas"][1], "#123456")
        self.assertEqual(table["accent"][0], "#ABCDEF")

    def test_a_role_missing_from_one_appearance_fails_the_build(self):
        palette = json.loads(PALETTE.read_text(encoding="utf-8"))
        del palette["dark"]["focus"]
        code, swift, stderr = self.generate_from(palette)
        self.assertNotEqual(code, 0)
        self.assertEqual(swift, "")
        self.assertIn("focus", stderr)

    def test_a_malformed_colour_fails_the_build(self):
        palette = json.loads(PALETTE.read_text(encoding="utf-8"))
        palette["light"]["danger"] = "red"
        code, swift, stderr = self.generate_from(palette)
        self.assertNotEqual(code, 0)
        self.assertEqual(swift, "")
        self.assertIn("light.danger", stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
