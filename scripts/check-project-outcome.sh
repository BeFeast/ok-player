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
windows_candidate_manifest_url="${OKP_PROJECT_HEALTH_WINDOWS_CANDIDATE_MANIFEST_URL:-https://github.com/$repository/releases/download/windows-candidate/candidate.windows.json}"
windows_candidate_feed_url="${OKP_PROJECT_HEALTH_WINDOWS_CANDIDATE_FEED_URL:-https://github.com/$repository/releases/download/windows-candidate/releases.win-candidate.json}"
max_lag="${OKP_PROJECT_HEALTH_MAX_UNPUBLISHED_MAIN_LAG_SECONDS:-7200}"
if [[ ! "$max_lag" =~ ^[0-9]+$ ]] || (( max_lag == 0 )); then
  echo "OKP_PROJECT_HEALTH_MAX_UNPUBLISHED_MAIN_LAG_SECONDS must be a positive integer" >&2
  exit 2
fi
source_ci_grace="${OKP_PROJECT_HEALTH_SOURCE_CI_GRACE_SECONDS:-900}"
if [[ ! "$source_ci_grace" =~ ^[0-9]+$ ]] || (( source_ci_grace == 0 )); then
  echo "OKP_PROJECT_HEALTH_SOURCE_CI_GRACE_SECONDS must be a positive integer" >&2
  exit 2
fi
max_schedule_age="${OKP_PROJECT_HEALTH_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS:-7200}"
if [[ ! "$max_schedule_age" =~ ^[0-9]+$ ]] || (( max_schedule_age == 0 )); then
  echo "OKP_PROJECT_HEALTH_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS must be a positive integer" >&2
  exit 2
fi
max_active_run_age="${OKP_PROJECT_HEALTH_MAX_CANDIDATE_RUN_AGE_SECONDS:-5400}"
if [[ ! "$max_active_run_age" =~ ^[0-9]+$ ]] || (( max_active_run_age == 0 )); then
  echo "OKP_PROJECT_HEALTH_MAX_CANDIDATE_RUN_AGE_SECONDS must be a positive integer" >&2
  exit 2
fi

scratch_prefix="ok-player-outcome-health"
if [[ -n "${OKP_SCRATCH_SESSION:-}" ]]; then
  [[ "$OKP_SCRATCH_SESSION" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || {
    echo "OKP_SCRATCH_SESSION must contain only letters, digits, dot, underscore, or hyphen" >&2
    exit 2
  }
  scratch_prefix="ok-player-${OKP_SCRATCH_SESSION}-outcome-health"
fi
work="$(mktemp -d -t "${scratch_prefix}.XXXXXX")" || exit 2
trap 'rm -rf -- "$work"' EXIT
: >"$work/windows-feed.json" || exit 2
: >"$work/windows-candidate-manifest.json" || exit 2
: >"$work/windows-candidate-feed.json" || exit 2
: >"$work/candidate-feed.json" || exit 2
: >"$work/linux-releases.json" || exit 2

source_error=""
main_sha=""
main_committed_at=""
main_observed_at=""
if main_commit="$(gh api "repos/$repository/commits/main" 2>/dev/null)" \
    && main_sha="$(jq -er '.sha | select(test("^[0-9a-fA-F]{40}$"))' <<<"$main_commit" 2>/dev/null)"; then
  main_committed_at="$(jq -r '.commit.committer.date // ""' <<<"$main_commit")"
else
  source_error="GitHub main commit query failed"
fi

if [[ -n "$main_sha" ]] \
    && push_events="$(gh api "repos/$repository/events?per_page=100" 2>/dev/null)"; then
  main_observed_at="$(jq -r --arg main_sha "$main_sha" '
    if type != "array" then error("not an array")
    else [
      .[]
      | select(
          .type == "PushEvent"
          and .payload.ref == "refs/heads/main"
          and .payload.head == $main_sha
        )
      | .created_at
      | select(type == "string" and length > 0)
    ][0] // ""
    end
  ' <<<"$push_events" 2>/dev/null)" || main_observed_at=""
fi

workflows='[]'
for workflow in CI Rust; do
  if run_list="$(gh run list --repo "$repository" --branch main --event push --workflow "$workflow" --limit 1 \
      --json workflowName,headSha,event,status,conclusion,createdAt,url 2>/dev/null)"; then
    if run="$(jq -c 'if type == "array" then .[0] // null else error("not an array") end' \
        <<<"$run_list" 2>/dev/null)"; then
      if [[ "$run" != "null" ]]; then
        workflows="$(jq --argjson run "$run" '. + [{
          name: $run.workflowName,
          head_sha: $run.headSha,
          event: $run.event,
          status: $run.status,
          conclusion: ($run.conclusion // ""),
          created_at_utc: ($run.createdAt // ""),
          url: $run.url
        }]' <<<"$workflows")"
      fi
    else
      source_error="${source_error:+$source_error; }GitHub $workflow workflow query returned malformed JSON"
    fi
  else
    source_error="${source_error:+$source_error; }GitHub $workflow workflow query failed"
  fi
done

if [[ -z "$main_observed_at" && -n "$main_sha" ]]; then
  main_observed_at="$(jq -r --arg main_sha "$main_sha" '[
    .[]
    | select(.head_sha == $main_sha)
    | .created_at_utc
    | select(type == "string" and length > 0)
  ] | min // ""' <<<"$workflows" 2>/dev/null)" || main_observed_at=""
fi

candidate_workflow_state=""
candidate_workflow_state_error=""
if candidate_workflow="$(gh api --cache 5m \
    "repos/$repository/actions/workflows/release-linux-candidate.yml" 2>/dev/null)"; then
  candidate_workflow_state="$(jq -r '.state // ""' <<<"$candidate_workflow")"
  if [[ -z "$candidate_workflow_state" ]]; then
    candidate_workflow_state_error="GitHub Linux Candidate workflow query omitted state"
  fi
else
  candidate_workflow_state_error="GitHub Linux Candidate workflow state query failed"
fi

candidate_active_run='null'
candidate_active_run_error=""
if candidate_runs="$(gh run list --repo "$repository" --branch main \
    --workflow "Linux Candidate" --limit 100 \
    --json headSha,event,status,createdAt,url 2>/dev/null)"; then
  if ! candidate_active_run="$(jq -c --arg main_sha "$main_sha" '
      if type != "array" then error("not an array")
      else [
        .[]
        | select((.headSha // "") == $main_sha)
        | select((.event // "") == "schedule" or (.event // "") == "workflow_dispatch")
        | select((.status // "") != "completed")
      ][0] // null
      end
    ' <<<"$candidate_runs" 2>/dev/null)"; then
    candidate_active_run='null'
    candidate_active_run_error="GitHub Linux Candidate active run query returned malformed JSON"
  fi
else
  candidate_active_run_error="GitHub Linux Candidate active run query failed"
fi

candidate_schedule_run='null'
candidate_schedule_error=""
candidate_consecutive_failed_runs=0
candidate_last_failed_gate=""
if candidate_schedule_runs="$(gh run list --repo "$repository" --branch main --event schedule \
    --status completed --workflow "Linux Candidate" --limit 100 \
    --json databaseId,headSha,event,status,conclusion,updatedAt,url 2>/dev/null)"; then
  if ! candidate_schedule_run="$(jq -c 'if type == "array" then .[0] // null else error("not an array") end' \
      <<<"$candidate_schedule_runs" 2>/dev/null)" \
      || ! candidate_consecutive_failed_runs="$(jq -r '
        if type != "array" then error("not an array")
        else reduce .[] as $run (
          {count: 0, stopped: false};
          if .stopped then .
          elif ($run.conclusion // "") == "failure" then .count += 1
          else .stopped = true
          end
        ) | .count
        end
      ' <<<"$candidate_schedule_runs" 2>/dev/null)"; then
    candidate_schedule_run='null'
    candidate_consecutive_failed_runs=0
    candidate_schedule_error="GitHub Linux Candidate completed schedule query returned malformed JSON"
  elif (( candidate_consecutive_failed_runs >= 2 )); then
    latest_failed_run_id="$(jq -r '.[0].databaseId // ""' <<<"$candidate_schedule_runs")"
    if [[ "$latest_failed_run_id" =~ ^[0-9]+$ ]] \
        && failed_log="$(gh run view "$latest_failed_run_id" --repo "$repository" --log-failed 2>/dev/null)"; then
      while IFS= read -r line; do
        if [[ "$line" =~ failed[[:space:]]+at[[:space:]]+gate[[:space:]]+([[:alnum:]_.-]+) ]]; then
          candidate_last_failed_gate="${BASH_REMATCH[1]}"
        fi
      done <<<"$failed_log"
    fi
  fi
else
  candidate_schedule_error="GitHub Linux Candidate completed schedule query failed"
fi

windows_candidate_workflow_state=""
windows_candidate_workflow_state_error=""
if windows_candidate_workflow="$(gh api --cache 5m \
    "repos/$repository/actions/workflows/release-windows-candidate.yml" 2>/dev/null)"; then
  windows_candidate_workflow_state="$(jq -r '.state // ""' <<<"$windows_candidate_workflow")"
  if [[ -z "$windows_candidate_workflow_state" ]]; then
    windows_candidate_workflow_state_error="GitHub Windows Candidate workflow query omitted state"
  fi
else
  windows_candidate_workflow_state_error="GitHub Windows Candidate workflow state query failed"
fi

windows_candidate_schedule_run='null'
windows_candidate_schedule_error=""
windows_candidate_consecutive_failed_runs=0
windows_candidate_last_failed_gate=""
if windows_candidate_schedule_runs="$(gh run list --repo "$repository" --branch main --event schedule \
    --status completed --workflow "Windows Candidate" --limit 100 \
    --json databaseId,headSha,event,status,conclusion,updatedAt,url 2>/dev/null)"; then
  if ! windows_candidate_schedule_run="$(jq -c 'if type == "array" then .[0] // null else error("not an array") end' \
      <<<"$windows_candidate_schedule_runs" 2>/dev/null)" \
      || ! windows_candidate_consecutive_failed_runs="$(jq -r '
        if type != "array" then error("not an array")
        else reduce .[] as $run (
          {count: 0, stopped: false};
          if .stopped then .
          elif ($run.conclusion // "") == "failure" then .count += 1
          else .stopped = true
          end
        ) | .count
        end
      ' <<<"$windows_candidate_schedule_runs" 2>/dev/null)"; then
    windows_candidate_schedule_run='null'
    windows_candidate_consecutive_failed_runs=0
    windows_candidate_schedule_error="GitHub Windows Candidate completed schedule query returned malformed JSON"
  elif (( windows_candidate_consecutive_failed_runs >= 2 )); then
    latest_windows_failed_run_id="$(jq -r '.[0].databaseId // ""' <<<"$windows_candidate_schedule_runs")"
    if [[ "$latest_windows_failed_run_id" =~ ^[0-9]+$ ]] \
        && windows_failed_jobs="$(gh run view "$latest_windows_failed_run_id" --repo "$repository" --json jobs 2>/dev/null)"; then
      windows_candidate_last_failed_gate="$(jq -r '
        [.jobs[]?.steps[]? | select((.conclusion // "") == "failure") | .name][0] // ""
      ' <<<"$windows_failed_jobs" 2>/dev/null || true)"
    fi
  fi
else
  windows_candidate_schedule_error="GitHub Windows Candidate completed schedule query failed"
fi

windows_ok=false
windows_error="Windows static feed request failed"
curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
  "$windows_url" >"$work/windows-feed.json" 2>"$work/windows-curl.err" &
windows_curl_pid=$!
curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
  "$windows_candidate_manifest_url" >"$work/windows-candidate-manifest.json" \
  2>"$work/windows-candidate-manifest-curl.err" &
windows_candidate_manifest_curl_pid=$!
curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
  "$windows_candidate_feed_url" >"$work/windows-candidate-feed.json" \
  2>"$work/windows-candidate-feed-curl.err" &
windows_candidate_feed_curl_pid=$!
curl --fail --silent --show-error --location --retry 2 --connect-timeout 10 --max-time 30 \
  "$candidate_url" >"$work/candidate-feed.json" 2>"$work/candidate-curl.err" &
candidate_curl_pid=$!

if wait "$windows_curl_pid"; then
  windows_ok=true
  windows_error=""
fi

windows_candidate_manifest_ok=false
windows_candidate_manifest_error="Windows candidate identity manifest request failed"
if wait "$windows_candidate_manifest_curl_pid"; then
  windows_candidate_manifest_ok=true
  windows_candidate_manifest_error=""
fi

windows_candidate_feed_ok=false
windows_candidate_feed_error="Windows candidate feed request failed"
if wait "$windows_candidate_feed_curl_pid"; then
  windows_candidate_feed_ok=true
  windows_candidate_feed_error=""
fi

candidate_ok=false
candidate_error="Linux candidate feed request failed"
if wait "$candidate_curl_pid"; then
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

windows_candidate_sha=""
windows_candidate_committed_at=""
windows_candidate_compare_status=""
windows_candidate_merge_base_sha=""
windows_candidate_first_unpublished_sha=""
windows_candidate_first_unpublished_at=""
windows_candidate_source_error=""
if $windows_candidate_manifest_ok; then
  windows_candidate_sha="$(jq -er '.source_sha | select(type == "string" and test("^[0-9a-fA-F]{40}$")) | ascii_downcase' \
    "$work/windows-candidate-manifest.json" 2>/dev/null || true)"
fi
if [[ -n "$windows_candidate_sha" && -n "$main_sha" ]]; then
  if windows_candidate_commit="$(gh api "repos/$repository/commits/$windows_candidate_sha" 2>/dev/null)"; then
    windows_candidate_committed_at="$(jq -r '.commit.committer.date // ""' <<<"$windows_candidate_commit")"
  else
    windows_candidate_source_error="GitHub Windows candidate source commit query failed"
  fi

  if [[ "$windows_candidate_sha" != "$main_sha" ]]; then
    if windows_candidate_comparison="$(gh api "repos/$repository/compare/$windows_candidate_sha...$main_sha?per_page=1" 2>/dev/null)"; then
      windows_candidate_compare_status="$(jq -r '.status // ""' <<<"$windows_candidate_comparison")"
      windows_candidate_merge_base_sha="$(jq -r '.merge_base_commit.sha // ""' <<<"$windows_candidate_comparison")"
      windows_candidate_first_unpublished_sha="$(jq -r '.commits[0].sha // ""' <<<"$windows_candidate_comparison")"
      windows_candidate_first_unpublished_at="$(jq -r '.commits[0].commit.committer.date // ""' <<<"$windows_candidate_comparison")"
      if [[ -z "$windows_candidate_compare_status" || -z "$windows_candidate_merge_base_sha" ]]; then
        windows_candidate_source_error="${windows_candidate_source_error:+$windows_candidate_source_error; }GitHub Windows candidate comparison omitted source graph evidence"
      fi
    else
      windows_candidate_source_error="${windows_candidate_source_error:+$windows_candidate_source_error; }GitHub Windows candidate-to-main comparison failed"
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
  --argjson source_ci_grace_seconds "$source_ci_grace" \
  --argjson max_unpublished_main_lag_seconds "$max_lag" \
  --argjson max_candidate_schedule_age_seconds "$max_schedule_age" \
  --argjson max_candidate_run_age_seconds "$max_active_run_age" \
  --arg main_sha "$main_sha" \
  --arg main_committed_at "$main_committed_at" \
  --arg main_observed_at "$main_observed_at" \
  --argjson workflows "$workflows" \
  --arg source_error "$source_error" \
  --arg candidate_workflow_state "$candidate_workflow_state" \
  --arg candidate_workflow_state_error "$candidate_workflow_state_error" \
  --argjson candidate_schedule_run "$candidate_schedule_run" \
  --arg candidate_schedule_error "$candidate_schedule_error" \
  --argjson candidate_active_run "$candidate_active_run" \
  --arg candidate_active_run_error "$candidate_active_run_error" \
  --argjson candidate_consecutive_failed_runs "$candidate_consecutive_failed_runs" \
  --arg candidate_last_failed_gate "$candidate_last_failed_gate" \
  --arg candidate_sha "$candidate_sha" \
  --arg candidate_committed_at "$candidate_committed_at" \
  --arg compare_status "$compare_status" \
  --arg merge_base_sha "$merge_base_sha" \
  --arg first_unpublished_sha "$first_unpublished_sha" \
  --arg first_unpublished_at "$first_unpublished_at" \
  --arg candidate_source_error "$candidate_source_error" \
  --arg windows_candidate_workflow_state "$windows_candidate_workflow_state" \
  --arg windows_candidate_workflow_state_error "$windows_candidate_workflow_state_error" \
  --argjson windows_candidate_schedule_run "$windows_candidate_schedule_run" \
  --arg windows_candidate_schedule_error "$windows_candidate_schedule_error" \
  --argjson windows_candidate_consecutive_failed_runs "$windows_candidate_consecutive_failed_runs" \
  --arg windows_candidate_last_failed_gate "$windows_candidate_last_failed_gate" \
  --arg windows_candidate_sha "$windows_candidate_sha" \
  --arg windows_candidate_committed_at "$windows_candidate_committed_at" \
  --arg windows_candidate_compare_status "$windows_candidate_compare_status" \
  --arg windows_candidate_merge_base_sha "$windows_candidate_merge_base_sha" \
  --arg windows_candidate_first_unpublished_sha "$windows_candidate_first_unpublished_sha" \
  --arg windows_candidate_first_unpublished_at "$windows_candidate_first_unpublished_at" \
  --arg windows_candidate_source_error "$windows_candidate_source_error" \
  --arg windows_url "$windows_url" \
  --arg windows_ok "$windows_ok" \
  --rawfile windows_body "$work/windows-feed.json" \
  --arg windows_error "$windows_error" \
  --arg windows_candidate_manifest_url "$windows_candidate_manifest_url" \
  --arg windows_candidate_manifest_ok "$windows_candidate_manifest_ok" \
  --rawfile windows_candidate_manifest_body "$work/windows-candidate-manifest.json" \
  --arg windows_candidate_manifest_error "$windows_candidate_manifest_error" \
  --arg windows_candidate_feed_url "$windows_candidate_feed_url" \
  --arg windows_candidate_feed_ok "$windows_candidate_feed_ok" \
  --rawfile windows_candidate_feed_body "$work/windows-candidate-feed.json" \
  --arg windows_candidate_feed_error "$windows_candidate_feed_error" \
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
    source_ci_grace_seconds: $source_ci_grace_seconds,
    max_unpublished_main_lag_seconds: $max_unpublished_main_lag_seconds,
    max_candidate_schedule_age_seconds: $max_candidate_schedule_age_seconds,
    max_candidate_run_age_seconds: $max_candidate_run_age_seconds,
    source: {
      head_sha: $main_sha,
      head_committed_at_utc: (if $main_committed_at == "" then null else $main_committed_at end),
      head_observed_at_utc: (if $main_observed_at == "" then null else $main_observed_at end),
      workflows: $workflows,
      candidate_workflow: {
        state: $candidate_workflow_state,
        state_error: (if $candidate_workflow_state_error == "" then null else $candidate_workflow_state_error end),
        latest_completed_schedule: (
          if $candidate_schedule_run == null then null
          else {
            head_sha: ($candidate_schedule_run.headSha // ""),
            event: ($candidate_schedule_run.event // ""),
            status: ($candidate_schedule_run.status // ""),
            conclusion: ($candidate_schedule_run.conclusion // ""),
            completed_at_utc: ($candidate_schedule_run.updatedAt // ""),
            url: ($candidate_schedule_run.url // "")
          }
          end
        ),
        schedule_error: (if $candidate_schedule_error == "" then null else $candidate_schedule_error end),
        latest_active_run: (
          if $candidate_active_run == null then null
          else {
            head_sha: ($candidate_active_run.headSha // ""),
            event: ($candidate_active_run.event // ""),
            status: ($candidate_active_run.status // ""),
            created_at_utc: ($candidate_active_run.createdAt // ""),
            url: ($candidate_active_run.url // "")
          }
          end
        ),
        active_run_error: (if $candidate_active_run_error == "" then null else $candidate_active_run_error end),
        consecutive_failed_runs: $candidate_consecutive_failed_runs,
        last_failed_gate: (if $candidate_last_failed_gate == "" then null else $candidate_last_failed_gate end)
      },
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
      windows_candidate_workflow: {
        state: $windows_candidate_workflow_state,
        state_error: (if $windows_candidate_workflow_state_error == "" then null else $windows_candidate_workflow_state_error end),
        latest_completed_schedule: (
          if $windows_candidate_schedule_run == null then null
          else {
            head_sha: ($windows_candidate_schedule_run.headSha // ""),
            event: ($windows_candidate_schedule_run.event // ""),
            status: ($windows_candidate_schedule_run.status // ""),
            conclusion: ($windows_candidate_schedule_run.conclusion // ""),
            completed_at_utc: ($windows_candidate_schedule_run.updatedAt // ""),
            url: ($windows_candidate_schedule_run.url // "")
          }
          end
        ),
        schedule_error: (if $windows_candidate_schedule_error == "" then null else $windows_candidate_schedule_error end),
        consecutive_failed_runs: $windows_candidate_consecutive_failed_runs,
        last_failed_gate: (if $windows_candidate_last_failed_gate == "" then null else $windows_candidate_last_failed_gate end)
      },
      windows_candidate: {
        candidate_sha: $windows_candidate_sha,
        candidate_committed_at_utc: (if $windows_candidate_committed_at == "" then null else $windows_candidate_committed_at end),
        comparison: (
          if $windows_candidate_compare_status == "" or $windows_candidate_merge_base_sha == "" then null
          else {
            status: $windows_candidate_compare_status,
            merge_base_sha: $windows_candidate_merge_base_sha,
            first_unpublished_commit: (
              if $windows_candidate_first_unpublished_sha == "" or $windows_candidate_first_unpublished_at == "" then null
              else {sha: $windows_candidate_first_unpublished_sha, committed_at_utc: $windows_candidate_first_unpublished_at}
              end
            )
          }
          end
        ),
        error: (if $windows_candidate_source_error == "" then null else $windows_candidate_source_error end)
      },
      error: (if $source_error == "" then null else $source_error end)
    },
    windows_feed: {
      url: $windows_url,
      body: (if $windows_ok == "true" then $windows_body else null end),
      error: (if $windows_error == "" then null else $windows_error end)
    },
    windows_candidate_manifest: {
      url: $windows_candidate_manifest_url,
      body: (if $windows_candidate_manifest_ok == "true" then $windows_candidate_manifest_body else null end),
      error: (if $windows_candidate_manifest_error == "" then null else $windows_candidate_manifest_error end)
    },
    windows_candidate_feed: {
      url: $windows_candidate_feed_url,
      body: (if $windows_candidate_feed_ok == "true" then $windows_candidate_feed_body else null end),
      error: (if $windows_candidate_feed_error == "" then null else $windows_candidate_feed_error end)
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
