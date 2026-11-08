#!/usr/bin/env python3
"""Run the GitHub-hosted macOS build for a commit that lives on Forgejo.

Forgejo is the canonical repository, but the Mac builds run on GitHub-hosted runners
(.github/workflows/macos-player-preview.yml). This bridge connects the two without a
GitHub pull request:

  scope   decide whether a change touches the macOS build at all
  run     push the commit to a bridge branch on the GitHub mirror (mac-ci/...), wait
          for the macOS workflow its push starts, mirror that run's conclusion as the
          exit code, download its artifact and delete the bridge branch

The Forgejo workflow .forgejo/workflows/macos-pr-check.yml calls both on every pull
request and every push to main, and `run` alone when it is started by hand. A worker
can call `run` by hand for a Forgejo pull request too; with --forgejo-status the result
is posted as a commit status on that commit.

Environment:
  MAC_CI_GITHUB_TOKEN  GitHub token able to push branches (including workflow
                       changes) and read and cancel Actions runs on the mirror
  FORGEJO_TOKEN        Forgejo token, only for --forgejo-status

The bridge never reports success unless the GitHub run concluded `success` and its
artifact holds the app archive with a matching SHA-256. A failed, cancelled or
timed-out run, or a run that never starts, is a failure. A missing token is a failure
too, unless `--without-token skip` is given: then nothing is built, the log carries a
warning and the exit code is 0, so a repository that has not configured the token yet
gets no failing check from it.
"""
import argparse
import hashlib
import io
import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

# What a change has to touch to need a Mac build. The pull_request paths of
# .github/workflows/macos-player-preview.yml list the same patterns.
MACOS_PATHS = (
    "macos/**",
    "scripts/build-macos-preview.sh",
    "scripts/generate-macos-*.py",
    "scripts/macos-*.sh",
    "scripts/tests/macos-*.sh",
    "scripts/mac-ci-bridge.py",
    "brand/**",
    "rust/Cargo.toml",
    "rust/Cargo.lock",
    "rust/crates/okp-ffi/**",
    "rust/crates/okp-session/**",
    "rust/crates/okp-mpv/**",
    "rust/crates/okp-core/**",
    ".github/workflows/macos-player-preview.yml",
    ".forgejo/workflows/macos-pr-check.yml",
)
WORKFLOW_PATH = ".github/workflows/macos-player-preview.yml"
APP_ARCHIVE = "ok-player-macos-arm64.zip"
CHECKSUMS = "SHA256SUMS.txt"
ARTIFACT_PREFIX = "macos-player-"
STATUS_CONTEXT = "macOS PR check / manual bridge"
SHA = re.compile(r"^[0-9a-f]{40}$")
NO_COMMIT = "0" * 40
BRIDGE_REF = re.compile(r"^mac-ci/[A-Za-z0-9._/-]+$")


def pattern_regex(pattern):
    parts = []
    for token in re.split(r"(\*\*|\*)", pattern):
        if token == "**":
            parts.append(".*")
        elif token == "*":
            parts.append("[^/]*")
        else:
            parts.append(re.escape(token))
    return re.compile("^" + "".join(parts) + "$")


PATH_REGEXES = tuple(pattern_regex(pattern) for pattern in MACOS_PATHS)


def macos_relevant(paths):
    """The changed paths that need a Mac build."""
    return [path for path in paths if any(regex.match(path) for regex in PATH_REGEXES)]


def changed_paths(repo, base, head):
    """Paths changed between the merge base of `base` and `head`, or None when unknown."""
    result = subprocess.run(["git", "-C", repo, "diff", "--name-only", f"{base}...{head}"],
                            capture_output=True, text=True)
    if result.returncode != 0:
        return None
    return [line for line in result.stdout.splitlines() if line]


def scope(args):
    # A manual run has no base, and a new branch's push event names the zero commit.
    # An empty base must not reach git: `...<head>` diffs against HEAD, which is <head>.
    no_base = args.base in ("", NO_COMMIT)
    paths = None if no_base else changed_paths(args.repo, args.base, args.head)
    if no_base:
        print("No base commit to compare with; building.")
        relevant = True
    elif paths is None:
        print(f"Cannot diff {args.base}...{head_label(args.head)}; building to be safe.")
        relevant = True
    else:
        matched = macos_relevant(paths)
        relevant = bool(matched)
        if matched:
            print("macOS-relevant changes:")
            for path in matched[:20]:
                print(f"  {path}")
            if len(matched) > 20:
                print(f"  ... and {len(matched) - 20} more")
        else:
            print(f"No macOS-relevant change among {len(paths)} changed file(s).")
    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as handle:
            handle.write(f"relevant={'true' if relevant else 'false'}\n")
    return 0


def head_label(sha):
    return sha[:12]


class BridgeError(Exception):
    pass


class GitHub:
    """The few GitHub REST calls the bridge needs."""

    def __init__(self, repo, token, api="https://api.github.com"):
        self.repo, self.token, self.api = repo, token, api.rstrip("/")

    def _open(self, method, url, body=None, accept="application/vnd.github+json"):
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("Accept", accept)
        request.add_header("X-GitHub-Api-Version", "2022-11-28")
        request.add_header("User-Agent", "ok-player-mac-ci-bridge")
        # Unredirected: artifact and log downloads redirect to signed storage URLs that
        # must not receive the token.
        request.add_unredirected_header("Authorization", f"Bearer {self.token}")
        if data is not None:
            request.add_header("Content-Type", "application/json")
        for attempt in range(4):
            try:
                with urllib.request.urlopen(request, timeout=120) as response:
                    return response.read()
            except urllib.error.HTTPError as error:
                if error.code < 500 or attempt == 3:
                    raise BridgeError(f"GitHub {method} {url} answered {error.code}") from None
            except (urllib.error.URLError, TimeoutError) as error:
                if attempt == 3:
                    raise BridgeError(f"GitHub {method} {url} failed: {error}") from None
            time.sleep(5 * (attempt + 1))
        raise AssertionError("unreachable")

    def _json(self, method, path, body=None):
        raw = self._open(method, f"{self.api}/repos/{self.repo}/{path}", body)
        return json.loads(raw) if raw else {}

    def runs(self, ref, sha):
        found = self._json("GET", f"actions/runs?branch={ref}&event=push&head_sha={sha}&per_page=50")
        return [run for run in found.get("workflow_runs", [])
                if run.get("path", "").split("@")[0] == WORKFLOW_PATH]

    def run(self, run_id):
        return self._json("GET", f"actions/runs/{run_id}")

    def cancel(self, run_id):
        self._json("POST", f"actions/runs/{run_id}/cancel")

    def artifacts(self, run_id):
        return self._json("GET", f"actions/runs/{run_id}/artifacts?per_page=100").get("artifacts", [])

    def download(self, url):
        return self._open("GET", url)

    def jobs(self, run_id):
        return self._json("GET", f"actions/runs/{run_id}/jobs?per_page=100").get("jobs", [])

    def job_log(self, job_id):
        return self._open("GET", f"{self.api}/repos/{self.repo}/actions/jobs/{job_id}/logs").decode(
            "utf-8", "replace")


class GitRemote:
    """The bridge branch on the mirror, pushed from the local clone."""

    def __init__(self, repo, url, token_variable="MAC_CI_GITHUB_TOKEN"):
        self.repo, self.url = repo, url
        # The helper reads the token from the environment when git asks for it, so the
        # token never appears in an argument list or a log line.
        self.auth = []
        if url.startswith("https://"):
            helper = f'!f() {{ echo username=x-access-token; echo "password=${{{token_variable}}}"; }}; f'
            self.auth = ["-c", "credential.helper=", "-c", f"credential.helper={helper}"]

    def _git(self, *args, check=True):
        result = subprocess.run(["git", "-C", self.repo, *self.auth, *args], capture_output=True, text=True)
        if check and result.returncode != 0:
            raise BridgeError(f"git {args[0]} failed: {result.stderr.strip()}")
        return result

    def sha_of(self, ref):
        output = self._git("ls-remote", self.url, f"refs/heads/{ref}").stdout.split()
        return output[0] if output else None

    def push(self, sha, ref):
        self._git("push", "--force", self.url, f"{sha}:refs/heads/{ref}")

    def delete(self, ref):
        self._git("push", self.url, f":refs/heads/{ref}")

    def delete_if_at(self, ref, sha):
        """Delete the branch unless something else has moved it on; True when deleted."""
        result = self._git("push", f"--force-with-lease=refs/heads/{ref}:{sha}", self.url,
                           f":refs/heads/{ref}", check=False)
        return result.returncode == 0


class Forgejo:
    def __init__(self, api, repo, token):
        self.api, self.repo, self.token = api.rstrip("/"), repo, token

    def status(self, sha, state, description, target_url=None):
        body = {"state": state, "context": STATUS_CONTEXT, "description": description[:140]}
        if target_url:
            body["target_url"] = target_url
        request = urllib.request.Request(f"{self.api}/repos/{self.repo}/statuses/{sha}",
                                         data=json.dumps(body).encode(), method="POST")
        request.add_header("Content-Type", "application/json")
        request.add_header("Authorization", f"token {self.token}")
        try:
            with urllib.request.urlopen(request, timeout=60):
                pass
        except (urllib.error.URLError, TimeoutError) as error:
            print(f"warning: could not post the Forgejo status: {error}", file=sys.stderr)


class Bridge:
    def __init__(self, github, remote, out=print, sleep=time.sleep, clock=time.monotonic,
                 poll_seconds=30, start_timeout=300, forgejo=None):
        self.github, self.remote, self.out = github, remote, out
        self.sleep, self.clock, self.poll_seconds = sleep, clock, poll_seconds
        self.start_timeout, self.forgejo = start_timeout, forgejo

    def run(self, sha, ref, timeout_seconds, download_dir=None):
        """Build `sha` through the bridge branch `ref`; return (passed, report lines)."""
        run_id, finished, report = None, False, [f"### macOS build of {sha}"]
        self._status(sha, "pending", "Waiting for the GitHub-hosted macOS build")
        try:
            known = {run["id"] for run in self.github.runs(ref, sha)}
            if self.remote.sha_of(ref) == sha:
                # A push that changes nothing starts no workflow run.
                self.remote.delete(ref)
            self.remote.push(sha, ref)
            self.out(f"Pushed {head_label(sha)} to the bridge branch {ref}.")
            run = self._wait_for_new_run(ref, sha, known)
            run_id = run["id"]
            self.out(f"GitHub run: {run['html_url']}")
            report.append(f"GitHub run: {run['html_url']}")
            self._status(sha, "pending", "GitHub-hosted macOS build running", run["html_url"])
            run = self._wait_for_completion(run, timeout_seconds)
            finished = run.get("status") == "completed"
            conclusion = run.get("conclusion") if finished else "timed out (bridge deadline)"
            report.append(f"Conclusion: {conclusion}")
            self.out(f"GitHub run concluded: {conclusion}")
            if not finished:
                self._cancel(run_id)
                finished = True
            passed = conclusion == "success"
            if conclusion != "success" and run.get("status") == "completed":
                self._print_failure(run_id)
            app_ok, lines = self._collect(run_id, download_dir)
            report += lines
            if passed and not app_ok:
                report.append("The run passed but its artifact has no verified app archive.")
                passed = False
            self._status(sha, "success" if passed else "failure",
                         f"macOS build {conclusion}", run["html_url"])
            return passed, report
        except BridgeError as error:
            report.append(f"Bridge error: {error}")
            self.out(f"error: {error}")
            self._status(sha, "error", f"macOS bridge error: {error}")
            return False, report
        finally:
            if run_id is not None and not finished:
                self._cancel(run_id)
            try:
                if self.remote.delete_if_at(ref, sha):
                    self.out(f"Deleted the bridge branch {ref}.")
                else:
                    self.out(f"Left the bridge branch {ref}: it is gone or a newer push moved it.")
            except BridgeError as error:
                self.out(f"warning: {error}")

    def _status(self, sha, state, description, target_url=None):
        if self.forgejo:
            self.forgejo.status(sha, state, description, target_url)

    def _cancel(self, run_id):
        try:
            self.github.cancel(run_id)
            self.out(f"Cancelled GitHub run {run_id}.")
        except BridgeError as error:
            self.out(f"warning: could not cancel GitHub run {run_id}: {error}")

    def _wait_for_new_run(self, ref, sha, known):
        deadline = self.clock() + self.start_timeout
        while True:
            fresh = [run for run in self.github.runs(ref, sha) if run["id"] not in known]
            if fresh:
                return max(fresh, key=lambda run: run["id"])
            if self.clock() >= deadline:
                raise BridgeError(f"no {WORKFLOW_PATH} run started for {ref} at {head_label(sha)}")
            self.sleep(10)

    def _wait_for_completion(self, run, timeout_seconds):
        deadline = self.clock() + timeout_seconds
        last = None
        while True:
            state = run.get("status")
            if state != last:
                self.out(f"GitHub run {run['id']}: {state}")
                last = state
            if state == "completed" or self.clock() >= deadline:
                return run
            self.sleep(self.poll_seconds)
            run = self.github.run(run["id"])

    def _print_failure(self, run_id):
        try:
            for job in self.github.jobs(run_id):
                self.out(f"Job '{job.get('name')}': {job.get('conclusion')}")
                for step in job.get("steps", []):
                    if step.get("conclusion") not in ("success", "skipped", None):
                        self.out(f"  step '{step.get('name')}': {step.get('conclusion')}")
                if job.get("conclusion") == "failure":
                    tail = self.github.job_log(job["id"]).splitlines()[-150:]
                    self.out(f"Last {len(tail)} log lines of '{job.get('name')}':")
                    for line in tail:
                        self.out(f"  {line}")
        except BridgeError as error:
            self.out(f"warning: could not read the failed job log: {error}")

    def _collect(self, run_id, download_dir):
        """Download the run's artifact; return (app archive verified, report lines)."""
        lines, verified = [], False
        artifacts = [a for a in self.github.artifacts(run_id)
                     if a.get("name", "").startswith(ARTIFACT_PREFIX) and not a.get("expired")]
        if not artifacts:
            return False, ["The run published no macOS artifact."]
        target = Path(download_dir) if download_dir else None
        for artifact in artifacts:
            with zipfile.ZipFile(io.BytesIO(self.github.download(artifact["archive_download_url"]))) as bundle:
                names = {Path(name).name: name for name in bundle.namelist()}
                if target:
                    target.mkdir(parents=True, exist_ok=True)
                    bundle.extractall(target)
                if APP_ARCHIVE in names and CHECKSUMS in names:
                    digest = hashlib.sha256(bundle.read(names[APP_ARCHIVE])).hexdigest()
                    listed = checksum_for(bundle.read(names[CHECKSUMS]).decode(), APP_ARCHIVE)
                    if listed == digest:
                        verified = True
                        lines.append(f"{APP_ARCHIVE} SHA-256: `{digest}`")
                    else:
                        lines.append(f"{APP_ARCHIVE} SHA-256 {digest} does not match {CHECKSUMS} ({listed}).")
                for extra in ("version.txt", "signing.txt"):
                    if extra in names:
                        text = bundle.read(names[extra]).decode("utf-8", "replace").strip()
                        lines.append(f"{extra}: {' '.join(text.split())}")
        if target:
            lines.append(f"Downloaded to {target}")
        return verified, lines


def checksum_for(text, name):
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 2 and Path(fields[-1].lstrip("*")).name == name:
            return fields[0].lower()
    return None


def run(args):
    if not SHA.match(args.sha):
        print(f"error: {args.sha!r} is not a full commit SHA", file=sys.stderr)
        return 2
    if not BRIDGE_REF.match(args.ref):
        print(f"error: the bridge branch must be under mac-ci/, not {args.ref!r}", file=sys.stderr)
        return 2
    token = os.environ.get("MAC_CI_GITHUB_TOKEN", "")
    forgejo = None
    if args.forgejo_status:
        if not args.forgejo_api or not os.environ.get("FORGEJO_TOKEN"):
            print("error: --forgejo-status needs --forgejo-api and FORGEJO_TOKEN", file=sys.stderr)
            return 2
        forgejo = Forgejo(args.forgejo_api, args.forgejo_repo, os.environ["FORGEJO_TOKEN"])
    if not token:
        # No status is posted: a missing credential says nothing about the commit.
        message = ("MAC_CI_GITHUB_TOKEN is not set, so this macOS-relevant change was not built. "
                   "See 'Forgejo pull request check' in macos/README.md for the one-time setup.")
        write_summary(args.summary, [f"### macOS build of {args.sha}", message])
        if args.without_token == "skip":
            print(f"::warning::{message}")
            return 0
        print(f"error: {message}", file=sys.stderr)
        return 1
    bridge = Bridge(GitHub(args.github_repo, token), GitRemote(args.repo, args.remote),
                    poll_seconds=args.poll_seconds, forgejo=forgejo)
    passed, report = bridge.run(args.sha, args.ref, args.timeout_minutes * 60, args.download_dir)
    write_summary(args.summary, report)
    print("\n".join(report))
    return 0 if passed else 1


def write_summary(path, lines):
    if path:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write("\n\n".join(lines) + "\n")


def terminate(signum, _frame):
    # Turn a cancellation into SystemExit, so the bridge branch is cleaned up and the
    # GitHub run is cancelled on the way out.
    raise SystemExit(128 + signum)


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    scope_parser = commands.add_parser("scope", help="does the change need a Mac build?")
    scope_parser.add_argument("--base", required=True,
                              help="base commit; empty or all zeros means none, which builds")
    scope_parser.add_argument("--head", required=True)
    scope_parser.add_argument("--repo", default=".")
    scope_parser.add_argument("--github-output", help="append relevant=true|false here")
    run_parser = commands.add_parser("run", help="build a commit on a GitHub-hosted Mac")
    run_parser.add_argument("--sha", required=True, help="full commit SHA, present in --repo")
    run_parser.add_argument("--ref", required=True, help="bridge branch, for example mac-ci/pr-123")
    run_parser.add_argument("--repo", default=".")
    run_parser.add_argument("--github-repo", default="BeFeast/ok-player")
    run_parser.add_argument("--remote", help="push URL of the mirror (default: its HTTPS URL)")
    run_parser.add_argument("--download-dir", help="extract the run's artifact here")
    run_parser.add_argument("--timeout-minutes", type=int, default=60)
    run_parser.add_argument("--poll-seconds", type=int, default=30)
    run_parser.add_argument("--summary", default=os.environ.get("GITHUB_STEP_SUMMARY"),
                            help="append a Markdown report here (default: $GITHUB_STEP_SUMMARY)")
    run_parser.add_argument("--forgejo-status", action="store_true",
                            help=f"post the result as the '{STATUS_CONTEXT}' commit status")
    run_parser.add_argument("--forgejo-api", help="Forgejo API base URL, ending in /api/v1")
    run_parser.add_argument("--forgejo-repo", default="BeFeast/ok-player")
    run_parser.add_argument("--without-token", choices=("fail", "skip"), default="fail",
                            help="without MAC_CI_GITHUB_TOKEN: fail (default), or build nothing "
                                 "and exit 0 with a warning")
    args = parser.parse_args(argv[1:])
    # A CI log should show progress while the bridge waits, not only when it exits.
    sys.stdout.reconfigure(line_buffering=True)
    if args.command == "scope":
        return scope(args)
    args.remote = args.remote or f"https://github.com/{args.github_repo}.git"
    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    return run(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
