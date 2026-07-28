#!/usr/bin/env bash
# The APT archive is built on the internal builder (the only machine that can
# reach the signing key) and travels to the hosted deploy job as an artifact.
# That handoff has ordering constraints which are invisible until a Pages
# deployment fails, so they are asserted here: the archive must be downloaded
# before anything reads or publishes it, and the key must never be handed to a
# hosted runner.
#
# Since issue #689 the archive carries two suites (stable and candidate). Both
# are produced and signed by ONE invocation of build-apt-repo.sh in the signing
# job — a second archive-building step, especially one on a hosted runner, would
# mean a second signing path. And the candidate suite is only as fresh as the
# last Pages deploy, so a rolling publication that does not trigger one leaves
# testers' `apt upgrade` on the previous build; that trigger is asserted too.
#
# "A step mentions the script" is not the same as "a step runs the script":
# `echo verify-apt-repo.sh disabled` mentions it. So the assertions below parse
# each run: block into commands and look at what is actually being executed.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WF="$ROOT/.github/workflows/publish-update-feeds.yml"
CANDIDATE_WF="$ROOT/.github/workflows/release-linux-candidate.yml"

python3 - "$WF" "$CANDIDATE_WF" <<'PY'
import os, re, shlex, sys, yaml

wf = yaml.safe_load(open(sys.argv[1]))
candidate_wf = yaml.safe_load(open(sys.argv[2]))
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

def commands(run):
    """Every command a run: block executes, as token lists."""
    for raw in (run or "").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        for part in re.split(r"&&|\|\||;|\|", line):
            try:
                tokens = shlex.split(part)
            except ValueError:
                continue
            # Skip leading VAR=value assignments and common prefixes, so
            # `sudo ./scripts/x.sh` and `FOO=1 ./scripts/x.sh` still count.
            i = 0
            while i < len(tokens) and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", tokens[i]):
                i += 1
            while i < len(tokens) and tokens[i] in ("sudo", "env", "bash", "sh", "exec"):
                i += 1
            if i < len(tokens):
                yield tokens[i:]

def invokes(job, script):
    """True when some step of the job actually executes <script>."""
    for step in job.get("steps", []):
        for tokens in commands(step.get("run")):
            if os.path.basename(tokens[0]) == script:
                return True
    return False

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

    # 5. apt itself is the only authority on whether the archive works, and it is
    #    asked once per deploy, on the hosted runner that holds the merged site.
    if not invokes(pub, "verify-apt-repo.sh"):
        fail.append("no step in publish-feeds executes verify-apt-repo.sh; an unverified archive would reach Pages")

# 6. One generator, two suites. Every suite in the archive must come out of the
#    same run of build-apt-repo.sh, so there is exactly one place where the
#    signing key is used and exactly one archive layout to reason about.
builders = sorted(name for name, job in jobs.items() if invokes(job, "build-apt-repo.sh"))
if builders != ["apt-repository"]:
    fail.append(
        "build-apt-repo.sh must be executed exactly once, by apt-repository; executed by "
        f"{builders!r}. Two invocations mean two signing paths and two archives."
    )

# 7. A rolling candidate publication has to reach the archive. publish-update-feeds is the only
#    writer of the Pages site, and replacing assets on the mutable linux-candidate release fires
#    no event it listens for, so without this call the APT candidate suite would only refresh when
#    something unrelated deployed. It must also be conditional: the candidate workflow runs every
#    15 minutes and most runs publish nothing.
refreshers = {
    name: job
    for name, job in candidate_wf["jobs"].items()
    if str(job.get("uses", "")).endswith("publish-update-feeds.yml")
}
if not refreshers:
    fail.append(
        "release-linux-candidate.yml never calls publish-update-feeds.yml; a new rolling candidate "
        "would not reach the APT candidate suite until an unrelated deploy happened to run"
    )
# 8. The refresh must not hold the native builder's concurrency group. A workflow-level group
#    covers every job in the run, so a Pages deploy that cannot start — signing runner offline,
#    secret store down — would stop candidate builds entirely. The QA lane must not be blocked by
#    the publication of its own archive, so the group belongs to the building job.
if candidate_wf.get("concurrency"):
    fail.append(
        "release-linux-candidate.yml declares a workflow-level concurrency group, which the feed "
        "refresh would hold for the length of a Pages deploy; scope it to the building job"
    )
if not candidate_wf["jobs"].get("build-and-publish", {}).get("concurrency"):
    fail.append(
        "the native build/publish job has no concurrency group; two builders would run at once"
    )

for name, job in refreshers.items():
    if job.get("secrets") != "inherit":
        fail.append(f"{name} must pass secrets: inherit, or the signing key cannot be fetched")
    condition = str(job.get("if", ""))
    produced = [n for n in ([job.get("needs")] if isinstance(job.get("needs"), str) else (job.get("needs") or []))]
    if not produced:
        fail.append(f"{name} does not depend on the publishing job, so it cannot know whether anything was published")
    if not condition or not any(f"needs.{n}.outputs" in condition for n in produced):
        fail.append(
            f"{name} must be gated on an output of {produced!r}; an unconditional refresh would "
            "redeploy Pages on every no-op scheduled run"
        )

if fail:
    for f in fail:
        print("FAIL:", f, file=sys.stderr)
    raise SystemExit(1)

print("ok: one generator builds both suites on the internal builder, the archive is downloaded "
      "and verified before it is deployed, and a rolling candidate publication refreshes it")
PY
