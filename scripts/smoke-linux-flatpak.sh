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
#
# "Still contains the integration" is enforced per patched file by
# scripts/flatpak_integration_markers.py: every path the patch touches must
# declare at least one marker of the behaviour it carries, and every declared
# marker must appear on a non-comment line of the pinned-and-patched tree. A
# deleted hunk therefore fails here rather than shipping a package that silently
# lost a feature, and a commented-out copy of the integration does not satisfy a
# marker. What a marker cannot prove is that the code it names is reachable or
# correct: it is a substring on a code line, nothing more. The offline build,
# the lifecycle lane, and the renderer smoke are what carry that weight.
#
# The same rule applies to the literal assertions this script makes about the
# workflow and the software-renderer script: they are matched against those
# files with whole-line comments removed, so a comment cannot stand in for the
# code being asserted. They remain substring assertions - they prove the text is
# present as code, not that it runs.
set -euo pipefail

# The gates below import and run repo-local Python; none of them should leave
# bytecode caches in a checkout.
export PYTHONDONTWRITEBYTECODE=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
CARGO_SOURCES="$ROOT/rust/packaging/flatpak/cargo-sources.json"
APP_PATCH="$ROOT/rust/packaging/flatpak/ok-player-flatpak.patch"
PATCHED_PATHS="$ROOT/rust/packaging/flatpak/patched-paths.txt"
BUILD_SCRIPT="$ROOT/scripts/build-flatpak-beta.sh"
LIFECYCLE_SCRIPT="$ROOT/scripts/smoke-linux-flatpak-lifecycle.sh"
LIFECYCLE_CONTROL_TEST="$ROOT/scripts/tests/flatpak-lifecycle-control.sh"
MARKER_CHECKER="$ROOT/scripts/flatpak_integration_markers.py"
CARGO_SOURCES_CHECKER="$ROOT/scripts/flatpak_cargo_sources.py"
MARKER_TEST="$ROOT/scripts/tests/flatpak-integration-markers.sh"
REPIN_SCRIPT="$ROOT/scripts/flatpak-repin.sh"
SOFTWARE_RENDER_SCRIPT="$ROOT/scripts/smoke-linux-software-renderer.sh"
WORKFLOW="$ROOT/.github/workflows/flatpak.yml"
GITIGNORE="$ROOT/.gitignore"

# How far the frozen pin may fall behind the default branch before it stops
# being a pin and becomes rot. The nightly re-pin pull request normally keeps
# the drift at one day; these bounds are the backstop for when it does not.
MAX_PIN_DRIFT_COMMITS="${OKP_FLATPAK_MAX_PIN_DRIFT_COMMITS:-50}"
MAX_PIN_DRIFT_DAYS="${OKP_FLATPAK_MAX_PIN_DRIFT_DAYS:-14}"

for tool in bash git python3 sed tar flatpak-builder desktop-file-validate appstreamcli; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

python3 - "$MANIFEST" "$CARGO_SOURCES" "$APP_PATCH" "$WORKFLOW" "$GITIGNORE" "$SOFTWARE_RENDER_SCRIPT" "$PATCHED_PATHS" "$ROOT" <<'PY'
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
root = Path(sys.argv[8])

sys.path.insert(0, str(root / "scripts"))
from flatpak_integration_markers import code_text  # noqa: E402

manifest = json.loads(manifest_path.read_text())
cargo_sources = json.loads(cargo_sources_path.read_text())
gitignore = gitignore_path.read_text().splitlines()

# Comments are not the thing being asserted. Matching against the comment-free
# text stops a description of a behaviour from standing in for the behaviour.
workflow_source = workflow_path.read_text()
workflow = code_text(workflow_path.name, workflow_source)
software_render_source = software_render_script_path.read_text()
software_render_script = code_text(
    software_render_script_path.name, software_render_source
)

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
assert "./scripts/tests/flatpak-lifecycle-control.sh" in workflow
assert "./scripts/tests/flatpak-integration-markers.sh" in workflow
assert "OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL: update-current" in workflow
# The negative control must assert the reason it failed, not merely that it
# failed: a preflight abort, a missing artifact manifest, or a renamed control
# id all produce a non-zero status without exercising anything.
assert '"$status" -ne 3' in workflow
assert "Flatpak lifecycle step update-current failed: deployed" in workflow
assert "./scripts/flatpak-repin.sh origin/main" in workflow
assert "gh pr create --base main" in workflow
assert "git push --force origin \"HEAD:refs/heads/$REPIN_BRANCH\"" in workflow
# A re-pin pull request that never starts CI is a proposal nobody can review.
assert "gh workflow run flatpak.yml --ref \"$REPIN_BRANCH\"" in workflow
assert "actions: write" in workflow
assert "artifacts/linux/flatpak/flatpak-lifecycle-ci.json" in workflow
assert "artifacts/manual-ui/linux-software-renderer-smoke/**" in workflow
assert re.search(r"apt-get install -y [^\n]*\bripgrep\b", workflow)

# The lane's own gates must be in the push path filter, or a change to a gate
# would not run the gate it changed. Asserted against the parsed workflow in
# scripts/tests/lane-split.Tests.sh (a required check on every pull request),
# not against the source text here: a textual count broke when the two-lane
# split (#727) removed the pull_request trigger, and this smoke sat red on
# main without running any of its real checks (#755).


def workflow_steps(text, job):
    """Return the steps of one job as (name, chunk) pairs, in file order.

    A hand parse rather than a YAML load, so this check needs nothing outside
    the standard library. It is fail-closed: when the shape it expects is not
    there it raises, instead of returning an empty step list that would make
    every per-step assertion below vacuously true.
    """
    lines = text.splitlines()
    if "jobs:" not in lines:
        raise SystemExit(f"{job}: the workflow has no top-level jobs: block")
    body = lines[lines.index("jobs:") + 1 :]
    in_job = False
    in_steps = False
    raw_steps = []
    for line in body:
        if re.fullmatch(r"  [A-Za-z0-9_.-]+:", line):
            if in_job:
                break
            in_job = line.strip().rstrip(":") == job
            continue
        if not in_job:
            continue
        if line == "    steps:":
            in_steps = True
            continue
        if not in_steps:
            continue
        if line.startswith("      - "):
            raw_steps.append([line])
        elif raw_steps and (line.startswith("        ") or not line.strip()):
            raw_steps[-1].append(line)
        elif not line.strip():
            continue
        else:
            # Anything else means the file no longer has the shape this parser
            # assumes. Stopping quietly here would drop the remaining steps out
            # of the guard check below.
            raise SystemExit(f"{job}: unexpected line in the steps block: {line!r}")
    if not raw_steps:
        raise SystemExit(f"{job}: no steps parsed from the workflow")
    parsed = []
    for raw in raw_steps:
        chunk = "\n".join(raw)
        # Anchored so that a "name:" nested under "with:" cannot be mistaken
        # for the step's own name.
        match = re.search(r"^(?:      - |        )name: (.*)$", chunk, re.MULTILINE)
        parsed.append((match.group(1).strip() if match else None, chunk))
    return parsed


steps = workflow_steps(workflow, "flatpak")

GUARD = "if: ${{ !cancelled() }}"
BUILD_STEP = "Build offline beta repository"
# Steps that must carry the guard, named individually so that renaming one is a
# failure rather than a silent removal from the requirement.
GUARDED_STEPS = {
    "Flatpak integration marker self-test",
    "Lifecycle negative control self-test",
    "Prepare XDG Pictures grant",
    "Repository lifecycle (install, update, rollback, restore, uninstall)",
    "Repository lifecycle negative control",
    "Packaged no-DRI software renderer smoke",
}
names = [name for name, _ in steps]
assert BUILD_STEP in names, names
absent = sorted(GUARDED_STEPS - set(names))
assert not absent, f"steps that must carry '{GUARD}' are absent from the job: {absent}"

# A failing gate must not silently skip the offline build, the delivery
# lifecycle, or the renderer smoke the way a plain step sequence would. This is
# asserted per step: a count would let one step lose its guard behind another
# gaining one. Everything after the build is covered positionally as well, so a
# newly added trailing step cannot arrive unguarded.
build_at = names.index(BUILD_STEP)
unguarded = [
    name or chunk.splitlines()[0].strip()
    for index, (name, chunk) in enumerate(steps)
    if (name in GUARDED_STEPS or index > build_at) and GUARD not in chunk
]
assert not unguarded, f"steps missing '{GUARD}': {unguarded}"

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
# An absence assertion is checked against the whole file on purpose: a mention
# in a comment is still a mention worth failing on.
assert "OKP_SOFTWARE_RENDER_PROBE" not in software_render_source
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

# Match patch sources structurally. Comparing whole dicts would let an extra
# key (say "use-git") hide a declared patch whose file does not exist.
patch_sources = [
    source
    for source in app["sources"]
    if isinstance(source, dict) and source.get("type") == "patch"
]
for source in patch_sources:
    referenced = manifest_path.parent / source["path"]
    assert referenced.is_file(), f"manifest declares a missing patch file: {source['path']}"
if app_patch_path.is_file():
    # The pin is an upstream commit that predates the Flatpak integration, so
    # the patch carries the difference.
    assert len(patch_sources) == 1, "exactly one integration patch source is expected"
    assert app["sources"][1] is patch_sources[0], "the patch must apply right after the git source"
    assert set(patch_sources[0]) == {"type", "path"}, patch_sources[0]
    assert patch_sources[0]["path"] == app_patch_path.name
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

  # A frozen pin is only defensible while it is close to the branch it freezes.
  # Past that it stops describing what the project ships and the "patch" becomes
  # an unreviewable catch-up diff.
  drift_commits="$(git -C "$ROOT" rev-list --count "$app_commit..origin/main")"
  pin_timestamp="$(git -C "$ROOT" show -s --format=%ct "$app_commit")"
  main_timestamp="$(git -C "$ROOT" show -s --format=%ct origin/main)"
  drift_days=$(( (main_timestamp - pin_timestamp) / 86400 ))
  if (( drift_days < 0 )); then
    drift_days=0
  fi
  if (( drift_commits > MAX_PIN_DRIFT_COMMITS || drift_days > MAX_PIN_DRIFT_DAYS )); then
    echo "Flatpak application pin $app_commit is stale: $drift_commits commits and $drift_days days behind origin/main (limits: $MAX_PIN_DRIFT_COMMITS commits, $MAX_PIN_DRIFT_DAYS days)." >&2
    echo "Run scripts/flatpak-repin.sh origin/main and commit the refreshed pin and patch." >&2
    exit 1
  fi
  echo "Flatpak application pin drift: $drift_commits commits, $drift_days days behind origin/main"
fi

# Materialize the exact tree flatpak-builder will build and prove the frozen
# patch still applies to it and still carries the Flatpak integration.
PINNED_TREE="$(mktemp -d)"
trap 'rm -rf "$PINNED_TREE"' EXIT
git -C "$ROOT" archive --format=tar "$app_commit" | tar -x -C "$PINNED_TREE"
if [[ -f "$APP_PATCH" ]]; then
  (cd "$PINNED_TREE" && git apply --binary "$APP_PATCH")
  python3 "$MARKER_CHECKER" "$PINNED_TREE" "$APP_PATCH"
else
  python3 "$MARKER_CHECKER" "$PINNED_TREE"
fi

# The vendor set is generated from one lockfile; the pin can move across a
# lockfile change without it. Catch that here rather than inside the offline
# build.
# Issue #743: every licence document the manifest installs must exist in the
# tree flatpak-builder actually builds. The application source is a pinned
# commit, so a document added to the repository after the pin reaches the build
# only as a declared source - an install command on its own turns the offline
# build into a "cannot stat" failure, which nothing here would have noticed.
python3 - "$MANIFEST" "$PINNED_TREE" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path, pinned_tree = Path(sys.argv[1]), Path(sys.argv[2])
app = json.loads(manifest_path.read_text())["modules"][0]

provided = set()
for source in app["sources"]:
    if isinstance(source, dict) and source.get("type") in {"file", "inline"}:
        provided.add(source.get("dest-filename") or Path(source["path"]).name)

installed, unresolved = [], []
for command in app["build-commands"]:
    match = re.match(r"install -Dm\d+ (\S+) /app/share/licenses/", command)
    if not match:
        continue
    document = match.group(1)
    installed.append(document)
    if document not in provided and not (pinned_tree / document).is_file():
        unresolved.append(document)

assert installed, "the Flatpak manifest installs no licence document at all"
if unresolved:
    raise SystemExit(
        "the Flatpak installs licence documents that are in neither the pinned "
        f"and patched tree nor a declared source: {sorted(unresolved)}"
    )
print(f"Flatpak licence documents resolve in the built tree: {sorted(installed)}")
PY

python3 "$CARGO_SOURCES_CHECKER" "$PINNED_TREE/rust/Cargo.lock" "$CARGO_SOURCES"

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
bash -n "$LIFECYCLE_CONTROL_TEST"
bash -n "$MARKER_TEST"
bash -n "$REPIN_SCRIPT"
bash -n "$SOFTWARE_RENDER_SCRIPT"

flatpak-builder --show-manifest "$MANIFEST" >/dev/null
desktop-file-validate "$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
appstreamcli validate --pedantic --no-color \
  "$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

echo "Flatpak manifest smoke passed: the pinned source applies its integration patch, native sources are pinned, the Cargo lock is offline, and sandbox permissions are valid"
