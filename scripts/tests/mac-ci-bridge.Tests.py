#!/usr/bin/env python3
"""The Forgejo-to-GitHub macOS bridge reports green only for a passed, verified build."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "mac-ci-bridge.py"
spec = importlib.util.spec_from_file_location("mac_ci_bridge", SCRIPT)
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)

SHA = "a" * 40
REF = "mac-ci/pr-7"


def artifact_zip(archive=b"app bytes", listed=None, name="ok-player-macos-arm64.zip"):
    """The GitHub artifact as the workflow uploads it."""
    digest = listed or hashlib.sha256(archive).hexdigest()
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as bundle:
        bundle.writestr(name, archive)
        bundle.writestr("SHA256SUMS.txt", f"{digest}  {name}\n")
        bundle.writestr("signing.txt", "mode=ad-hoc\n")
        bundle.writestr("snapshots/window-light.png", b"png")
    return buffer.getvalue()


class FakeGitHub:
    """A run that appears after the push and moves through the given states."""

    def __init__(self, states, artifact=None, start_after=1, existing=()):
        self.states = list(states)
        self.artifact = artifact
        self.start_after = start_after
        self.existing = [self._run(run_id, "completed", "success") for run_id in existing]
        self.polls = 0
        self.cancelled = []

    @staticmethod
    def _run(run_id, status, conclusion=None):
        return {"id": run_id, "status": status, "conclusion": conclusion,
                "html_url": f"https://github.example/runs/{run_id}"}

    def _current(self):
        status, conclusion = self.states[min(self.polls, len(self.states) - 1)]
        return self._run(100, status, conclusion)

    def runs(self, ref, sha):
        self.start_after -= 1
        return self.existing + ([self._current()] if self.start_after < 0 else [])

    def run(self, run_id):
        self.polls += 1
        return self._current()

    def cancel(self, run_id):
        self.cancelled.append(run_id)

    def artifacts(self, run_id):
        if self.artifact is None:
            return []
        return [{"name": f"macos-player-{SHA}-1", "archive_download_url": "zip", "expired": False}]

    def download(self, url):
        return self.artifact

    def jobs(self, run_id):
        return [{"id": 1, "name": "Build Apple Silicon app (GitHub-hosted)", "conclusion": "failure",
                 "steps": [{"name": "Build app", "conclusion": "failure"}]}]

    def job_log(self, job_id):
        return "error: cannot find 'PaletteRole' in scope\n"


class FakeRemote:
    def __init__(self, existing_sha=None):
        self.refs = {REF: existing_sha} if existing_sha else {}
        self.events = []

    def sha_of(self, ref):
        return self.refs.get(ref)

    def push(self, sha, ref):
        self.events.append(("push", sha, ref))
        self.refs[ref] = sha

    def delete(self, ref):
        self.events.append(("delete", ref))
        self.refs.pop(ref, None)

    def delete_if_at(self, ref, sha):
        if self.refs.get(ref) == sha:
            self.delete(ref)
            return True
        return False


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


def run_bridge(github, remote=None, timeout=3600, download_dir=None):
    clock, lines = Clock(), []
    remote = remote or FakeRemote()
    subject = bridge.Bridge(github, remote, out=lines.append, sleep=clock.sleep, clock=clock,
                            poll_seconds=30, start_timeout=300)
    passed, report = subject.run(SHA, REF, timeout, download_dir)
    return passed, report, lines, remote


IN_PROGRESS = [("queued", None), ("in_progress", None)]


class BridgeOutcomeTests(unittest.TestCase):
    def test_passed_run_with_a_verified_archive_is_green_and_cleans_up(self):
        github = FakeGitHub(IN_PROGRESS + [("completed", "success")], artifact_zip())
        with tempfile.TemporaryDirectory() as scratch:
            passed, report, _, remote = run_bridge(github, download_dir=scratch)
            self.assertTrue(Path(scratch, "ok-player-macos-arm64.zip").exists())
            self.assertTrue(Path(scratch, "snapshots", "window-light.png").exists())
        self.assertTrue(passed, report)
        digest = hashlib.sha256(b"app bytes").hexdigest()
        self.assertIn(f"ok-player-macos-arm64.zip SHA-256: `{digest}`", report)
        self.assertIn("GitHub run: https://github.example/runs/100", report)
        self.assertIn("signing.txt: mode=ad-hoc", report)
        self.assertEqual(remote.events, [("push", SHA, REF), ("delete", REF)])

    def test_failed_run_is_red_even_with_an_archive_and_prints_the_failing_log(self):
        github = FakeGitHub(IN_PROGRESS + [("completed", "failure")], artifact_zip())
        passed, report, lines, remote = run_bridge(github)
        self.assertFalse(passed)
        self.assertIn("Conclusion: failure", report)
        self.assertIn("  error: cannot find 'PaletteRole' in scope", lines)
        self.assertNotIn(REF, remote.refs)

    def test_cancelled_run_is_red(self):
        passed, report, _, _ = run_bridge(FakeGitHub([("completed", "cancelled")], artifact_zip()))
        self.assertFalse(passed)
        self.assertIn("Conclusion: cancelled", report)

    def test_passed_run_whose_archive_does_not_match_its_checksum_is_red(self):
        github = FakeGitHub([("completed", "success")], artifact_zip(listed="0" * 64))
        passed, report, _, _ = run_bridge(github)
        self.assertFalse(passed)
        self.assertTrue(any("does not match" in line for line in report), report)

    def test_passed_run_without_an_artifact_is_red(self):
        passed, report, _, _ = run_bridge(FakeGitHub([("completed", "success")], artifact=None))
        self.assertFalse(passed)
        self.assertIn("The run published no macOS artifact.", report)

    def test_run_that_outlasts_the_deadline_is_red_and_cancelled(self):
        github = FakeGitHub([("in_progress", None)], artifact_zip())
        passed, report, _, remote = run_bridge(github, timeout=600)
        self.assertFalse(passed)
        self.assertIn("Conclusion: timed out (bridge deadline)", report)
        self.assertEqual(github.cancelled, [100])
        self.assertNotIn(REF, remote.refs)

    def test_run_that_never_starts_is_red(self):
        github = FakeGitHub([("completed", "success")], artifact_zip(), start_after=10_000)
        passed, report, _, remote = run_bridge(github)
        self.assertFalse(passed)
        self.assertTrue(any("no .github/workflows/macos-player-preview.yml run started" in line
                            for line in report), report)
        self.assertNotIn(REF, remote.refs)

    def test_an_earlier_green_run_of_the_same_commit_is_not_taken_for_this_one(self):
        # The new run shows up only on the third poll, so the first polls see nothing
        # but the earlier green run 99 and must keep waiting.
        github = FakeGitHub([("completed", "failure")], artifact_zip(), existing=[99], start_after=3)
        passed, report, _, _ = run_bridge(github)
        self.assertFalse(passed)
        self.assertIn("GitHub run: https://github.example/runs/100", report)
        self.assertNotIn("GitHub run: https://github.example/runs/99", report)

    def test_branch_already_at_the_commit_is_recreated_so_the_push_starts_a_run(self):
        remote = FakeRemote(existing_sha=SHA)
        run_bridge(FakeGitHub([("completed", "success")], artifact_zip()), remote=remote)
        self.assertEqual(remote.events[:2], [("delete", REF), ("push", SHA, REF)])

    def test_branch_moved_by_a_newer_push_is_left_alone(self):
        remote = FakeRemote()

        class Superseded(FakeGitHub):
            def run(self, run_id):
                remote.refs[REF] = "b" * 40
                return super().run(run_id)

        run_bridge(Superseded([("in_progress", None), ("completed", "cancelled")]), remote=remote)
        self.assertEqual(remote.refs[REF], "b" * 40)


class CommandLineTests(unittest.TestCase):
    def run_script(self, *args, env_extra=None, cwd=None):
        env = {key: value for key, value in os.environ.items()
               if key not in ("MAC_CI_GITHUB_TOKEN", "GITHUB_STEP_SUMMARY")}
        env.update(env_extra or {})
        return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True,
                              env=env, cwd=cwd)

    def test_missing_token_fails_before_pushing_anything(self):
        with tempfile.TemporaryDirectory() as scratch:
            summary = Path(scratch, "summary.md")
            result = self.run_script("run", "--sha", SHA, "--ref", REF, "--repo", scratch,
                                     "--remote", str(Path(scratch, "nowhere.git")), "--summary", str(summary))
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("MAC_CI_GITHUB_TOKEN is not set", result.stderr)
            self.assertIn("MAC_CI_GITHUB_TOKEN is not set", summary.read_text())

    def test_missing_token_with_skip_builds_nothing_and_passes_with_a_warning(self):
        with tempfile.TemporaryDirectory() as scratch:
            summary, mirror = Path(scratch, "summary.md"), Path(scratch, "mirror.git")
            subprocess.run(["git", "init", "-q", "--bare", str(mirror)], check=True)
            result = self.run_script("run", "--sha", SHA, "--ref", REF, "--repo", scratch,
                                     "--remote", str(mirror), "--summary", str(summary),
                                     "--without-token", "skip")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("::warning::MAC_CI_GITHUB_TOKEN is not set", result.stdout)
            self.assertIn("was not built", summary.read_text())
            self.assertEqual(git(mirror, "for-each-ref"), "", "nothing may be pushed")

    def test_bridge_branch_outside_mac_ci_is_refused(self):
        result = self.run_script("run", "--sha", SHA, "--ref", "main",
                                 env_extra={"MAC_CI_GITHUB_TOKEN": "unused"})
        self.assertEqual(result.returncode, 2)


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True).stdout.strip()


def commit(repo, path, text):
    file = Path(repo, path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(text)
    git(repo, "add", path)
    git(repo, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", path)
    return git(repo, "rev-parse", "HEAD")


class ScopeTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.repo = Path(self.scratch.name, "repo")
        self.repo.mkdir()
        git(self.repo, "init", "-q")
        self.base = commit(self.repo, "README.md", "base\n")

    def tearDown(self):
        self.scratch.cleanup()

    def relevant(self, head, base=None):
        output = Path(self.scratch.name, "output")
        output.write_text("")
        result = subprocess.run([sys.executable, str(SCRIPT), "scope", "--repo", str(self.repo),
                                 "--base", self.base if base is None else base, "--head", head,
                                 "--github-output", str(output)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return output.read_text().strip()

    def test_docs_only_change_needs_no_mac_build(self):
        head = commit(self.repo, "docs/notes.md", "docs\n")
        self.assertEqual(self.relevant(head), "relevant=false")

    def test_shell_change_needs_a_mac_build(self):
        head = commit(self.repo, "macos/Sources/OKPlayerShell/Player.swift", "swift\n")
        self.assertEqual(self.relevant(head), "relevant=true")

    def test_unknown_base_builds_to_be_safe(self):
        head = commit(self.repo, "docs/notes.md", "docs\n")
        self.assertEqual(self.relevant(head, base="f" * 40), "relevant=true")

    def test_run_without_a_base_builds(self):
        # A manual run passes an empty base; a new branch's push event the zero commit.
        head = commit(self.repo, "docs/notes.md", "docs\n")
        self.assertEqual(self.relevant(head, base=""), "relevant=true")
        self.assertEqual(self.relevant(head, base="0" * 40), "relevant=true")

    def test_push_compares_with_the_previous_tip(self):
        before = commit(self.repo, "macos/README.md", "mac\n")
        head = commit(self.repo, "docs/notes.md", "docs\n")
        self.assertEqual(self.relevant(head, base=before), "relevant=false")
        self.assertEqual(self.relevant(head, base=self.base), "relevant=true")

    def test_path_set(self):
        needs = [
            "macos/Package.swift", "scripts/build-macos-preview.sh", "scripts/generate-macos-icon.py",
            "scripts/macos-signing-identity.sh", "scripts/tests/macos-signing-identity.Tests.sh",
            "scripts/mac-ci-bridge.py", "brand/palette.json",
            "rust/Cargo.lock", "rust/crates/okp-session/src/lib.rs", "rust/crates/okp-ffi/src/live.rs",
            "rust/crates/okp-mpv/src/player.rs", "rust/crates/okp-core/src/branding.rs",
            ".github/workflows/macos-player-preview.yml", ".forgejo/workflows/macos-pr-check.yml",
        ]
        skips = [
            "docs/qa-records/x.md", "scripts/generate-chrome-icons.py", "rust/crates/okp-linux-gtk/src/main.rs",
            "src/OkPlayer.App/App.xaml.cs", ".github/workflows/rust.yml",
            # Both run on Linux in the Rust job, not in the Mac build.
            "scripts/tests/macos-bundle.Tests.py", "scripts/tests/mac-ci-bridge.Tests.py",
        ]
        self.assertEqual(bridge.macos_relevant(needs), needs)
        self.assertEqual(bridge.macos_relevant(skips), [])

    def test_github_workflow_triggers_on_the_same_paths(self):
        try:
            import yaml
        except ImportError:
            self.skipTest("PyYAML is not installed")
        workflow = yaml.safe_load((ROOT / ".github/workflows/macos-player-preview.yml").read_text())
        triggers = workflow.get("on", workflow.get(True))
        self.assertEqual(set(triggers["pull_request"]["paths"]), set(bridge.MACOS_PATHS))
        self.assertEqual(triggers["push"]["branches"], ["mac-ci/**"])


class GitRemoteTests(unittest.TestCase):
    def test_branch_is_deleted_only_while_it_still_points_at_the_bridged_commit(self):
        with tempfile.TemporaryDirectory() as scratch:
            repo, mirror = Path(scratch, "repo"), Path(scratch, "mirror.git")
            repo.mkdir()
            git(repo, "init", "-q")
            first = commit(repo, "a.txt", "1\n")
            second = commit(repo, "a.txt", "2\n")
            subprocess.run(["git", "init", "-q", "--bare", str(mirror)], check=True)
            remote = bridge.GitRemote(str(repo), str(mirror))
            remote.push(first, REF)
            self.assertEqual(remote.sha_of(REF), first)
            remote.push(second, REF)
            self.assertFalse(remote.delete_if_at(REF, first))
            self.assertEqual(remote.sha_of(REF), second)
            self.assertTrue(remote.delete_if_at(REF, second))
            self.assertIsNone(remote.sha_of(REF))


if __name__ == "__main__":
    unittest.main()
