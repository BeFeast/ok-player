#!/usr/bin/env bash
# Reject pull requests whose base-to-head tree contains no file changes.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 BASE_SHA HEAD_SHA" >&2
  exit 2
fi

base_sha="$1"
head_sha="$2"

for revision in "$base_sha" "$head_sha"; do
  if ! git cat-file -e "${revision}^{commit}" 2>/dev/null; then
    echo "Unable to inspect pull request revision: $revision" >&2
    exit 2
  fi
done

set +e
git diff --quiet "${base_sha}...${head_sha}" --
diff_status=$?
set -e

case "$diff_status" in
  0)
    message="Pull request has no file changes. Empty traceability commits cannot be reviewed by Greptile. QA and acceptance work must add docs/qa-records/YYYY-MM-DD-issue-NNN.md."
    echo "::error title=Pull request has no reviewable file changes::$message" >&2
    echo "$message" >&2
    exit 1
    ;;
  1)
    echo "Pull request contains reviewable file changes."
    ;;
  *)
    echo "Unable to compare pull request revisions $base_sha and $head_sha." >&2
    exit 2
    ;;
esac
