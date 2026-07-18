#!/usr/bin/env python3
"""Run a command as a Linux child subreaper and drain its process tree."""

from __future__ import annotations

import ctypes
import os
import signal
import subprocess
import sys
import time
from pathlib import Path


PR_SET_CHILD_SUBREAPER = 36
TERM_GRACE_SECONDS = 2.0
KILL_GRACE_SECONDS = 2.0


def enable_child_subreaper() -> None:
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))


def process_parents() -> dict[int, int]:
    parents: dict[int, int] = {}
    for stat_path in Path("/proc").glob("[0-9]*/stat"):
        try:
            stat = stat_path.read_text()
            fields = stat.rsplit(") ", 1)[1].split()
            parents[int(stat_path.parent.name)] = int(fields[1])
        except (IndexError, OSError, ValueError):
            continue
    return parents


def descendant_pids(root_pid: int) -> list[int]:
    parents = process_parents()
    descendants: set[int] = set()
    frontier = [root_pid]
    while frontier:
        parent = frontier.pop()
        children = [pid for pid, ppid in parents.items() if ppid == parent]
        for child in children:
            if child not in descendants:
                descendants.add(child)
                frontier.append(child)
    return sorted(descendants, reverse=True)


def reap_exited_children() -> None:
    while True:
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            return
        if pid == 0:
            return


def signal_descendants(root_pid: int, sig: signal.Signals) -> None:
    for pid in descendant_pids(root_pid):
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            continue


def drain_descendants(root_pid: int, sig: signal.Signals, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while True:
        signal_descendants(root_pid, sig)
        reap_exited_children()
        if not descendant_pids(root_pid):
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.02)


def normalized_status(returncode: int) -> int:
    if returncode >= 0:
        return returncode
    return 128 - returncode


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: run-linux-child-subreaper.py <command> [args...]", file=sys.stderr)
        return 2

    try:
        enable_child_subreaper()
    except OSError as error:
        print(f"Failed to enable Linux child subreaper: {error}", file=sys.stderr)
        return 125

    command = subprocess.Popen(sys.argv[1:])
    command_status = normalized_status(command.wait())
    root_pid = os.getpid()

    if not drain_descendants(root_pid, signal.SIGTERM, TERM_GRACE_SECONDS):
        if not drain_descendants(root_pid, signal.SIGKILL, KILL_GRACE_SECONDS):
            remaining = descendant_pids(root_pid)
            print(
                "Failed to reap isolated session descendants: "
                + " ".join(str(pid) for pid in remaining),
                file=sys.stderr,
            )
            return 125

    reap_exited_children()
    return command_status


if __name__ == "__main__":
    sys.exit(main())
