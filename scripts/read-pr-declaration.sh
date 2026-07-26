#!/usr/bin/env bash
# Write a pull request's current title and body to TITLE/BODY files for the
# unfinished-work gate.
#
# The title and body are read live from the API rather than taken from the
# workflow event payload: a re-run replays the payload that fired the run, so an
# author who removed a marker would stay blocked until they pushed a commit.
#
# usage: read-pr-declaration.sh PR_NUMBER OUTPUT_DIR
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 PR_NUMBER OUTPUT_DIR" >&2
  exit 2
fi

pr_number="$1"
output_dir="$2"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"

mkdir -p "$output_dir"
gh api "repos/${repository}/pulls/${pr_number}" --jq '.title // ""' >"${output_dir}/pr-title.txt"
gh api "repos/${repository}/pulls/${pr_number}" --jq '.body // ""' >"${output_dir}/pr-body.txt"

echo "Read declaration for pull request #${pr_number}."
