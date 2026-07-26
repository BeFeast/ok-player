#!/usr/bin/env bash
# Root-free static validation for the Flatpak manifest and its offline source lock.
#
# The application source is a frozen pair: a pinned upstream commit plus the
# checked-in integration patch. This check validates that pair on its own terms
# - the patch must apply cleanly to the pinned tree and the applied result must
# still contain the Flatpak integration - instead of comparing the patch to the
# current working tree. A working-tree comparison would turn every later change
# to a patched file into an unrelated packaging failure; keeping the pin fresh
# is the job of scripts/flatpak-repin.sh and its scheduled pull request.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
CARGO_SOURCES="$ROOT/rust/packaging/flatpak/cargo-sources.json"
APP_PATCH="$ROOT/rust/packaging/flatpak/ok-player-flatpak.patch"
PATCHED_PATHS="$ROOT/rust/packaging/flatpak/patched-paths.txt"
BUILD_SCRIPT="$ROOT/scripts/build-flatpak-beta.sh"
LIFECYCLE_SCRIPT="$ROOT/scripts/smoke-linux-flatpak-lifecycle.sh"
REPIN_SCRIPT="$ROOT/scripts/flatpak-repin.sh"
SOFTWARE_RENDER_SCRIPT="$ROOT/scripts/smoke-linux-software-renderer.sh"
WORKFLOW="$ROOT/.github/workflows/flatpak.yml"
GITIGNORE="$ROOT/.gitignore"

for tool in bash git python3 sed tar flatpak-builder desktop-file-validate appstreamcli; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

python3 - "$MANIFEST" "$CARGO_SOURCES" "$APP_PATCH" "$WORKFLOW" "$GITIGNORE" "$SOFTWARE_RENDER_SCRIPT" "$PATCHED_PATHS" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
cargo_sources_path = Path(sys.argv[2])
app_patch_path = Path(sys.argv[3])
workflow_path = Path(sys.argv[4])
gitignore_path = Path(sys.argv[5])
software_render_script_path = Path(sys.argv[6])
patched_paths_path = Path(sys.argv[7])
manifest = json.loads(manifest_path.read_text())
cargo_sources = json.loads(cargo_sources_path.read_text())
workflow = workflow_path.read_text()
gitignore = gitignore_path.read_text().splitlines()
software_render_script = software_render_script_path.read_text()

assert "ref: ${{ github.event.pull_request.head.sha || github.sha }}" in workflow
assert "OKP_ACCEPTANCE_SOURCE_COMMIT: ${{ github.event.pull_request.head.sha || github.sha }}" in workflow
assert "OKP_FLATPAK_ARTIFACT_MANIFEST: artifacts/linux/flatpak/flatpak-beta-artifact.json" in workflow
# The merged default branch must build the artifact users install, and the
# nightly run keeps that true even when no packaging file changed.
assert "  push:\n    branches:\n      - main\n" in workflow
assert "  schedule:\n    - cron:" in workflow
assert "Packaged no-DRI software renderer smoke" in workflow
assert "flatpak run --user --nodevice=dri" in workflow
assert 'xdg-user-dirs-update --set PICTURES "$HOME/Pictures"' in workflow
assert "./scripts/smoke-linux-flatpak-lifecycle.sh" in workflow
assert "OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL: update-current" in workflow
assert "./scripts/flatpak-repin.sh origin/main" in workflow
assert "gh pr create --base main" in workflow
assert "git push --force origin \"HEAD:refs/heads/$REPIN_BRANCH\"" in workflow
assert "artifacts/linux/flatpak/flatpak-lifecycle-ci.json" in workflow
assert "artifacts/manual-ui/linux-software-renderer-smoke/**" in workflow
assert re.search(r"apt-get install -y [^\n]*\bripgrep\b", workflow)
# A failing gate must not silently skip the offline build, the delivery
# lifecycle, or the renderer smoke the way a plain step sequence would.
assert workflow.count("if: ${{ !cancelled() }}") >= 5
assert "mapped_gtk_player_window=pass" in software_render_script
assert '[[ "$window_map_state" == "IsViewable" ]]' in software_render_script
assert "non_trivial_geometry=pass" in software_render_script
assert "visible_video_region=pass" in software_render_script
assert "backend=libmpv-software" in software_render_script
assert "gtk_scene_renderer=cairo" in software_render_script
assert "software_pixel_format=bgr0" in software_render_script
assert "dri_fd_count=" in software_render_script
assert "command -v magick" in software_render_script
assert "image_convert()" in software_render_script
assert "image_compare()" in software_render_script
assert "convert compare" in software_render_script
assert "flatpak ps --columns=child-pid,application" in software_render_script
assert 'child_process" == "ok-player"' in software_render_script
assert "xdg-user-dir PICTURES" in software_render_script
assert '"map_state": window_map_state' in software_render_script
assert '"screenshots": {' in software_render_script
assert '"source_commit": source_commit' in software_render_script
assert "screenshot_sha256=" in software_render_script
assert "later_screenshot_sha256=" in software_render_script
assert 'sys.argv[2]: "<repo>"' in software_render_script
assert 'sys.argv[3]: "<home>"' in software_render_script
assert "probe_backend=not-run" in software_render_script
assert "OKP_SOFTWARE_RENDER_PROBE" not in software_render_script
assert "Renderer policy: mode=software-no-dri" in software_render_script
assert "flatpak-software-renderer-validate" in software_render_script
assert '--source-commit "$SOURCE_COMMIT"' in software_render_script
assert "/.flatpak-builder/" in gitignore

assert manifest["app-id"] == "com.befeast.okplayer"
assert manifest["runtime"] == "org.gnome.Platform"
assert manifest["runtime-version"] == "50"
assert "org.freedesktop.Sdk.Extension.rust-stable" in manifest["sdk-extensions"]

extensions = manifest["add-extensions"]
codecs = extensions["org.freedesktop.Platform.codecs-extra"]
assert codecs["version"] == "25.08-extra"
assert codecs["directory"] == "lib/codecs-extra"
assert codecs["add-ld-path"] == "."

permissions = set(manifest["finish-args"])
required = {
    "--socket=fallback-x11",
    "--socket=wayland",
    "--socket=pulseaudio",
    "--device=dri",
    "--filesystem=xdg-pictures:rw",
    "--own-name=org.mpris.MediaPlayer2.okplayer",
}
assert required <= permissions
assert "--device=all" not in permissions
assert "--filesystem=host" not in permissions
assert "--filesystem=home" not in permissions
assert "LIBGL_ALWAYS_SOFTWARE" not in json.dumps(manifest)

app = manifest["modules"][0]
assert "cargo --offline build --locked" in app["build-commands"][0]
assert app["build-options"]["env"]["CARGO_NET_OFFLINE"] == "true"
assert app["build-options"]["env"]["OKP_BUILD_VERSION"] == "0.11.0-beta.1"
assert app["build-options"]["env"]["OKP_BUILD_SHA"] == "flatpak-beta"
assert "cargo-sources.json" in app["sources"]
app_source = app["sources"][0]
assert app_source["type"] == "git"
assert app_source["url"] == "https://github.com/BeFeast/ok-player.git"
assert len(app_source.get("commit", "")) == 40

patched_paths = [
    line.split("#", 1)[0].strip()
    for line in patched_paths_path.read_text().splitlines()
]
patched_paths = [line for line in patched_paths if line]
assert patched_paths, "the Flatpak patch must declare the paths it carries"

patch_source = {"type": "patch", "path": app_patch_path.name}
patch_sources = [source for source in app["sources"] if source == patch_source]
if app_patch_path.is_file():
    # The pin is an upstream commit that predates the Flatpak integration, so
    # the patch carries the difference.
    assert app["sources"][1] == patch_source
    app_patch = app_patch_path.read_text()
    touched = {
        match.group(1)
        for match in re.finditer(r"^diff --git a/(\S+) b/\S+$", app_patch, re.MULTILINE)
    }
    assert touched <= set(patched_paths), sorted(touched - set(patched_paths))
    index_lines = [line for line in app_patch.splitlines() if line.startswith("index ")]
    assert index_lines
    assert all(
        re.fullmatch(r"index [0-9a-f]{40}\.\.[0-9a-f]{40}(?: [0-7]{6})?", line)
        for line in index_lines
    ), "Flatpak patch must use full Git object IDs"
else:
    # After the integration lands upstream the patch collapses to nothing and
    # the manifest must stop declaring it.
    assert not patch_sources, "the manifest declares a patch source with no patch file"

def assert_portable_meson_libdir(module):
    if module.get("buildsystem") == "meson":
        assert "--libdir=lib" in module.get("config-opts", []), module["name"]
    for child in module.get("modules", []):
        assert_portable_meson_libdir(child)

for module in app.get("modules", []):
    assert_portable_meson_libdir(module)

native_sources = []
def collect(value):
    if isinstance(value, dict):
        if value.get("type") in {"archive", "git"} and value.get("url"):
            native_sources.append(value)
        for child in value.values():
            collect(child)
    elif isinstance(value, list):
        for child in value:
            collect(child)
collect(app["sources"][0])
collect(app.get("modules", []))
for source in native_sources:
    if source["type"] == "archive":
        assert source.get("sha256"), source
    else:
        assert source.get("commit"), source

archives = [source for source in cargo_sources if source.get("type") == "archive"]
assert archives, "Cargo source lock contains no crates"
assert all(source.get("sha256") for source in archives)
assert cargo_sources[-1].get("dest-filename") == "config"
assert "replace-with = \"vendored-sources\"" in cargo_sources[-1].get("contents", "")
PY

app_commit="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["modules"][0]["sources"][0]["commit"])' "$MANIFEST")"
git -C "$ROOT" cat-file -e "${app_commit}^{tree}"

# flatpak-builder clones the public GitHub URL, so a pin that only exists in a
# local checkout would be unbuildable for everyone else.
if git -C "$ROOT" rev-parse --verify --quiet origin/main >/dev/null; then
  git -C "$ROOT" merge-base --is-ancestor "$app_commit" origin/main || {
    echo "Flatpak application pin $app_commit is not published on origin/main" >&2
    exit 1
  }
fi

# Materialize the exact tree flatpak-builder will build and prove the frozen
# patch still applies to it and still carries the Flatpak integration.
PINNED_TREE="$(mktemp -d)"
trap 'rm -rf "$PINNED_TREE"' EXIT
git -C "$ROOT" archive --format=tar "$app_commit" | tar -x -C "$PINNED_TREE"
if [[ -f "$APP_PATCH" ]]; then
  (cd "$PINNED_TREE" && git apply --binary "$APP_PATCH")
fi

python3 - "$PINNED_TREE" <<'PY'
import sys
from pathlib import Path

tree = Path(sys.argv[1])
# Markers of the packaged behaviour the Flatpak lane promises. They are checked
# against the pinned-and-patched tree, which is exactly what gets built.
required = {
    "THIRD-PARTY-NOTICES.md": [
        "source-built rendering library in the Flatpak beta",
        "libplacebo",
    ],
    "rust/crates/okp-core/src/linux_renderer.rs": [
        "software-no-dri",
        "pub const fn select_linux_renderer(flatpak: bool, dri_accessible: bool)",
    ],
    "rust/crates/okp-core/src/playback_failure.rs": [
        "org.freedesktop.Platform.codecs-extra",
        "CodecEnvironment::Flatpak",
    ],
    "rust/crates/okp-mpv/src/player.rs": [
        "MPV_RENDER_PARAM_ADVANCED_CONTROL",
    ],
    "rust/crates/okp-linux-gtk/src/updates.rs": [
        "Managed by Flatpak",
    ],
}
missing = []
for relative, markers in required.items():
    path = tree / relative
    if not path.is_file():
        missing.append(f"{relative}: missing from the pinned and patched tree")
        continue
    text = path.read_text()
    for marker in markers:
        if marker not in text:
            missing.append(f"{relative}: missing {marker!r}")
if missing:
    raise SystemExit(
        "The pinned Flatpak source does not carry the integration it packages:\n"
        + "\n".join(missing)
    )
PY

schema_version="$(sed -n 's/^pub const FLATPAK_LIFECYCLE_EVIDENCE_SCHEMA_VERSION: u32 = \([0-9]\+\);$/\1/p' \
  "$ROOT/rust/crates/okp-core/src/acceptance_evidence.rs")"
[[ -n "$schema_version" ]] || {
  echo "Could not read the Flatpak lifecycle evidence schema version" >&2
  exit 1
}
grep -q "\"schema_version\": $schema_version," "$LIFECYCLE_SCRIPT" || {
  echo "The lifecycle lane emits a schema version other than $schema_version" >&2
  exit 1
}

bash -n "$BUILD_SCRIPT"
bash -n "$LIFECYCLE_SCRIPT"
bash -n "$REPIN_SCRIPT"
bash -n "$SOFTWARE_RENDER_SCRIPT"

flatpak-builder --show-manifest "$MANIFEST" >/dev/null
desktop-file-validate "$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
appstreamcli validate --pedantic --no-color \
  "$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

echo "Flatpak manifest smoke passed: the pinned source applies its integration patch, native sources are pinned, the Cargo lock is offline, and sandbox permissions are valid"
