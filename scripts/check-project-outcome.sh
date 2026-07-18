#!/usr/bin/env bash
# Read-only project outcome checker (issue #412).
#
# The shell gathers remote evidence; okp-core evaluates the contract. This
# command never publishes a feed, creates a release, or changes repository
# settings. A fixture snapshot can be evaluated without network access:
#   scripts/check-project-outcome.sh --snapshot path/to/snapshot.json

set -uo pipefail

root="$(cd -- "$(dirname -- "$0")/.." && pwd)"

resolve_health_bin() {
  local mode="$1"
  if [[ -n "${OKP_PROJECT_HEALTH_BIN:-}" ]]; then
    [[ -x "$OKP_PROJECT_HEALTH_BIN" ]] || {
      echo "OKP_PROJECT_HEALTH_BIN is not executable" >&2
      return 2
    }
    printf '%s\n' "$OKP_PROJECT_HEALTH_BIN"
    return
  fi

  local candidate
  for candidate in \
    "$root/rust/target/release/okp-candidate" \
    "$root/rust/target/debug/okp-candidate"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
  done

  if [[ "$mode" == "snapshot" ]]; then
    echo "offline snapshot evaluation requires executable OKP_PROJECT_HEALTH_BIN or a prebuilt rust/target/{release,debug}/okp-candidate" >&2
  else
    echo "live project health requires executable OKP_PROJECT_HEALTH_BIN or a prebuilt rust/target/{release,debug}/okp-candidate; run the repository build outside the bounded healthcheck" >&2
  fi
  return 2
}

if [[ "${1:-}" == "--snapshot" ]]; then
  [[ $# -eq 2 ]] || { echo "usage: $0 --snapshot PATH" >&2; exit 2; }
  health_bin="$(resolve_health_bin snapshot)" || exit 2
  exec "$health_bin" project-health --snapshot "$2"
fi
[[ $# -eq 0 ]] || { echo "usage: $0 [--snapshot PATH]" >&2; exit 2; }

health_bin="$(resolve_health_bin live)" || exit 2

for command in gh curl jq date mktemp; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command is unavailable: $command" >&2
    exit 2
  }
done

repository="${OKP_PROJECT_HEALTH_REPOSITORY:-BeFeast/ok-player}"
candidate_url="${OKP_PROJECT_HEALTH_CANDIDATE_URL:-https://github.com/$repository/releases/download/linux-candidate/candidate.linux.json}"
windows_url="${OKP_PROJECT_HEALTH_WINDOWS_URL:-https://befeast.github.io/ok-player/updates/win/releases.win.json}"
max_lag="${OKP_PROJECT_HEALTH_MAX_UNPUBLISHED_MAIN_LAG_SECONDS:-7200}"
if [[ ! "$max_lag" =~ ^[0-9]+$ ]] || (( max_lag == 0 )); then
  echo "OKP_PROJECT_HEALTH_MAX_UNPUBLISHED_MAIN_LAG_SECONDS must be a positive integer" >&2
  exit 2
fi

work="$(mktemp -d)" || exit 2
trap 'rm -rf -- "$work"' EXIT
: >"$work/windows-feed.json" || exit 2
: >"$work/candidate-feed.json" || exit 2
: >"$work/linux-releases.json" || exit 2

source_error=""
main_sha=""
if main_commit="$(gh api "repos/$repository/commits/main" 2>/dev/null)" \
    && main_sha="$(jq -er '.sha | select(test("^[0-9a-fA-F]{40}$"))' <<<"$main_commit" 2>/dev/null)"; then
  :
else
  source_error="GitHub main commit query failed"
fi

workflows='[]'
for workflow in CI Rust; do
  if run="$(gh run list --repo "$repository" --branch main --event push --workflow "$workflow" --limit 1 \
      --json workflowName,headSha,event,status,conclusion,url 2>/dev/null | jq '.[0] // empty')" \
      && [[ -n "$run" ]]; then
    workflows="$(jq --argjson run "$run" '. + [{
      name: $run.workflowName,
      head_sha: $run.headSha,
      event: $run.event,
      status: $run.status,
      conclusion: ($run.conclusion // ""),
      url: $run.url
    }]' <<<"$workflows")"
  else
    source_error="${source_error:+$source_error; }GitHub $workflow workflow query failed"
  fi
done

windows_ok=false
windows_error="Windows static feed request failed"
if curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
    "$windows_url" >"$work/windows-feed.json" 2>"$work/windows-curl.err"; then
  windows_ok=true
  windows_error=""
fi

candidate_ok=false
candidate_error="Linux candidate feed request failed"
if curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
    "$candidate_url" >"$work/candidate-feed.json" 2>"$work/candidate-curl.err"; then
  candidate_ok=true
  candidate_error=""
fi

candidate_sha=""
candidate_committed_at=""
compare_status=""
merge_base_sha=""
first_unpublished_sha=""
first_unpublished_at=""
candidate_source_error=""
if $candidate_ok; then
  candidate_sha="$(jq -er '.commit_sha | select(type == "string" and test("^[0-9a-fA-F]{40}$")) | ascii_downcase' \
    "$work/candidate-feed.json" 2>/dev/null || true)"
fi
if [[ -n "$candidate_sha" && -n "$main_sha" ]]; then
  if candidate_commit="$(gh api "repos/$repository/commits/$candidate_sha" 2>/dev/null)"; then
    candidate_committed_at="$(jq -r '.commit.committer.date // ""' <<<"$candidate_commit")"
  else
    candidate_source_error="GitHub candidate source commit query failed"
  fi

  if [[ "$candidate_sha" != "$main_sha" ]]; then
    if comparison="$(gh api "repos/$repository/compare/$candidate_sha...$main_sha?per_page=1" 2>/dev/null)"; then
      compare_status="$(jq -r '.status // ""' <<<"$comparison")"
      merge_base_sha="$(jq -r '.merge_base_commit.sha // ""' <<<"$comparison")"
      first_unpublished_sha="$(jq -r '.commits[0].sha // ""' <<<"$comparison")"
      first_unpublished_at="$(jq -r '.commits[0].commit.committer.date // ""' <<<"$comparison")"
      if [[ -z "$compare_status" || -z "$merge_base_sha" ]]; then
        candidate_source_error="${candidate_source_error:+$candidate_source_error; }GitHub comparison omitted source graph evidence"
      fi
    else
      candidate_source_error="${candidate_source_error:+$candidate_source_error; }GitHub candidate-to-main comparison failed"
    fi
  fi
fi

stable_ok=false
stable_error="GitHub permanent Linux release query failed"
if gh api "repos/$repository/releases?per_page=100" >"$work/linux-releases.json" 2>/dev/null; then
  stable_ok=true
  stable_error=""
fi

jq -n \
  --argjson checked_at_unix "$(date -u +%s)" \
  --argjson max_unpublished_main_lag_seconds "$max_lag" \
  --arg main_sha "$main_sha" \
  --argjson workflows "$workflows" \
  --arg source_error "$source_error" \
  --arg candidate_sha "$candidate_sha" \
  --arg candidate_committed_at "$candidate_committed_at" \
  --arg compare_status "$compare_status" \
  --arg merge_base_sha "$merge_base_sha" \
  --arg first_unpublished_sha "$first_unpublished_sha" \
  --arg first_unpublished_at "$first_unpublished_at" \
  --arg candidate_source_error "$candidate_source_error" \
  --arg windows_url "$windows_url" \
  --arg windows_ok "$windows_ok" \
  --rawfile windows_body "$work/windows-feed.json" \
  --arg windows_error "$windows_error" \
  --arg candidate_url "$candidate_url" \
  --arg candidate_ok "$candidate_ok" \
  --rawfile candidate_body "$work/candidate-feed.json" \
  --arg candidate_error "$candidate_error" \
  --arg stable_url "https://api.github.com/repos/$repository/releases?per_page=100" \
  --arg stable_ok "$stable_ok" \
  --rawfile stable_body "$work/linux-releases.json" \
  --arg stable_error "$stable_error" '
  {
    checked_at_unix: $checked_at_unix,
    max_unpublished_main_lag_seconds: $max_unpublished_main_lag_seconds,
    source: {
      head_sha: $main_sha,
      workflows: $workflows,
      candidate: {
        candidate_sha: $candidate_sha,
        candidate_committed_at_utc: (if $candidate_committed_at == "" then null else $candidate_committed_at end),
        comparison: (
          if $compare_status == "" or $merge_base_sha == "" then null
          else {
            status: $compare_status,
            merge_base_sha: $merge_base_sha,
            first_unpublished_commit: (
              if $first_unpublished_sha == "" or $first_unpublished_at == "" then null
              else {sha: $first_unpublished_sha, committed_at_utc: $first_unpublished_at}
              end
            )
          }
          end
        ),
        error: (if $candidate_source_error == "" then null else $candidate_source_error end)
      },
      error: (if $source_error == "" then null else $source_error end)
    },
    windows_feed: {
      url: $windows_url,
      body: (if $windows_ok == "true" then $windows_body else null end),
      error: (if $windows_error == "" then null else $windows_error end)
    },
    candidate_feed: {
      url: $candidate_url,
      body: (if $candidate_ok == "true" then $candidate_body else null end),
      error: (if $candidate_error == "" then null else $candidate_error end)
    },
    stable_linux_releases: {
      url: $stable_url,
      body: (if $stable_ok == "true" then $stable_body else null end),
      error: (if $stable_error == "" then null else $stable_error end)
    }
  }
' >"$work/project-health-snapshot.json" || exit 2

exec "$health_bin" project-health --snapshot "$work/project-health-snapshot.json"
