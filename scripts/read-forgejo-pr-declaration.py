#!/usr/bin/env python3
"""Read the current Forgejo PR declaration, failing closed on API errors."""
import json
import os
from pathlib import Path
import sys
from urllib.parse import urlparse
from urllib.request import Request, urlopen


def main():
    if len(sys.argv) != 3 or not sys.argv[1].isdigit():
        raise SystemExit("usage: read-forgejo-pr-declaration.py PR_NUMBER OUTPUT_DIR")
    api = os.environ["FORGEJO_API_URL"].rstrip("/")
    if urlparse(api).scheme != "https":
        raise SystemExit("Forgejo API requires HTTPS")
    repository = os.environ["GITHUB_REPOSITORY"]
    if len(repository.split("/")) != 2 or any(
        c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_./"
        for c in repository
    ):
        raise SystemExit("Invalid repository name")
    request = Request(
        f"{api}/repos/{repository}/pulls/{sys.argv[1]}",
        headers={"Authorization": f"token {os.environ['FORGEJO_TOKEN']}"},
    )
    with urlopen(request, timeout=30) as response:
        declaration = json.load(response)
    if not isinstance(declaration.get("title"), str):
        raise SystemExit("Forgejo response has no PR title")
    body = declaration.get("body") or ""
    if not isinstance(body, str):
        raise SystemExit("Forgejo response has invalid PR body")
    output = Path(sys.argv[2])
    output.mkdir(parents=True, exist_ok=True)
    (output / "pr-title.txt").write_text(declaration["title"] + "\n")
    (output / "pr-body.txt").write_text(body + "\n")
    print(f"Read declaration for pull request #{sys.argv[1]}.")


if __name__ == "__main__":
    main()
