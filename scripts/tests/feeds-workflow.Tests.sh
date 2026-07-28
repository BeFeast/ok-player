#!/usr/bin/env bash
# The APT archive is built on the internal builder (the only machine that can
# reach the signing key) and travels to the hosted deploy job as an artifact.
# That handoff has ordering constraints which are invisible until a Pages
# deployment fails, so they are asserted here: the archive must be downloaded
# before anything reads or publishes it, and the key must never be handed to a
# hosted runner.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WF="$ROOT/.github/workflows/publish-update-feeds.yml"

python3 - "$WF" <<'PY'
import sys, yaml

wf = yaml.safe_load(open(sys.argv[1]))
jobs = wf["jobs"]
fail = []

def step_names(job):
    return [s.get("name") or s.get("uses") or "" for s in job["steps"]]

def index_of(names, needle):
    for i, n in enumerate(names):
        if needle in n:
            return i
    return -1

def body(job):
    return yaml.safe_dump(job)

# 1. The signing job runs where the key host resolves, and nowhere else.
apt = jobs.get("apt-repository")
if apt is None:
    fail.append("the apt-repository job is gone; signing would move back to a hosted runner")
else:
    runs_on = apt.get("runs-on")
    if not (isinstance(runs_on, list) and "self-hosted" in runs_on):
        fail.append(f"apt-repository must run on a self-hosted runner, got {runs_on!r}")

pub = jobs.get("publish-feeds")
if pub is None:
    fail.append("publish-feeds job is gone")
else:
    # 2. The deploy job waits for the archive.
    needs = pub.get("needs")
    needs = [needs] if isinstance(needs, str) else (needs or [])
    if "apt-repository" not in needs:
        fail.append(f"publish-feeds must depend on apt-repository, needs={needs!r}")

    names = step_names(pub)
    dl = index_of(names, "download-artifact")
    verify = index_of(names, "Verify the APT repository")
    upload = index_of(names, "upload-pages-artifact")

    # 3. Nothing may read or publish the archive before it exists locally.
    if dl < 0:
        fail.append("publish-feeds never downloads the apt-repository artifact")
    else:
        if verify >= 0 and dl > verify:
            fail.append("the archive is verified before it is downloaded: the verifier reads _site/apt, which only the artifact provides")
        if upload >= 0 and dl > upload:
            fail.append("the site is uploaded before the archive is merged into it")

    # 4. The signing secrets must never reach a hosted runner.
    if "INFISICAL_CLIENT_SECRET" in body(pub):
        fail.append("publish-feeds references the Infisical credentials; the key must stay on the internal builder")

if fail:
    for f in fail:
        print("FAIL:", f, file=sys.stderr)
    raise SystemExit(1)

print("ok: signing stays on the internal builder and the archive is downloaded before it is read or deployed")
PY
