#!/usr/bin/env bash
# Keep one Maestro worker alive when project health, pause state, and queue policy allow it.
set -euo pipefail
umask 077

fail() {
  echo "worker-floor: $*" >&2
  exit 2
}

note() {
  echo "worker-floor: $*"
}

is_positive_integer() {
  [[ "$1" =~ ^[0-9]+$ ]] && (( 10#$1 > 0 ))
}

repository="${OKP_WORKER_FLOOR_REPOSITORY:-BeFeast/ok-player}"
project="${OKP_WORKER_FLOOR_PROJECT:-ok-player}"
ready_label="${OKP_WORKER_FLOOR_READY_LABEL:-ok-player-ready}"
blocked_label="${OKP_WORKER_FLOOR_BLOCKED_LABEL:-blocked}"
maestro_config="${OKP_WORKER_FLOOR_CONFIG:-}"
fleet_url="${OKP_WORKER_FLOOR_FLEET_URL:-}"
source_repository="${OKP_WORKER_FLOOR_SOURCE_REPOSITORY:-}"
state_dir="${OKP_WORKER_FLOOR_STATE_DIR:-}"
quarantine_dir="${OKP_WORKER_FLOOR_QUARANTINE_DIR:-}"
qa_hold_issues="${OKP_WORKER_FLOOR_QA_HOLD_ISSUES:-545 546}"
quarantine_roots="${OKP_WORKER_FLOOR_QUARANTINE_ROOTS:-.agents .claude .cursor}"
expected_branch="${OKP_WORKER_FLOOR_SOURCE_BRANCH:-main}"
fetch_timeout="${OKP_WORKER_FLOOR_FETCH_TIMEOUT_SECONDS:-8}"
issue_limit="${OKP_WORKER_FLOOR_ISSUE_LIMIT:-1000}"
maestro_bin="${OKP_WORKER_FLOOR_MAESTRO_BIN:-maestro}"

[[ -n "$repository" ]] || fail "OKP_WORKER_FLOOR_REPOSITORY must not be empty"
[[ -n "$project" ]] || fail "OKP_WORKER_FLOOR_PROJECT must not be empty"
[[ -n "$ready_label" ]] || fail "OKP_WORKER_FLOOR_READY_LABEL must not be empty"
[[ -n "$blocked_label" ]] || fail "OKP_WORKER_FLOOR_BLOCKED_LABEL must not be empty"
[[ -n "$maestro_config" ]] || fail "OKP_WORKER_FLOOR_CONFIG is required"
[[ "$maestro_config" == /* ]] || fail "OKP_WORKER_FLOOR_CONFIG must be absolute"
[[ -f "$maestro_config" ]] || fail "OKP_WORKER_FLOOR_CONFIG must name a regular file"
[[ -n "$fleet_url" ]] || fail "OKP_WORKER_FLOOR_FLEET_URL is required"
[[ -n "$source_repository" ]] || fail "OKP_WORKER_FLOOR_SOURCE_REPOSITORY is required"
[[ "$source_repository" == /* ]] || fail "OKP_WORKER_FLOOR_SOURCE_REPOSITORY must be absolute"
[[ -n "$state_dir" ]] || fail "OKP_WORKER_FLOOR_STATE_DIR is required"
[[ "$state_dir" == /* ]] || fail "OKP_WORKER_FLOOR_STATE_DIR must be absolute"
[[ -z "$quarantine_dir" || "$quarantine_dir" == /* ]] || \
  fail "OKP_WORKER_FLOOR_QUARANTINE_DIR must be absolute"
[[ -n "$expected_branch" ]] || fail "OKP_WORKER_FLOOR_SOURCE_BRANCH must not be empty"
is_positive_integer "$fetch_timeout" || fail "OKP_WORKER_FLOOR_FETCH_TIMEOUT_SECONDS must be a positive integer"
is_positive_integer "$issue_limit" || fail "OKP_WORKER_FLOOR_ISSUE_LIMIT must be a positive integer"

for command in curl date flock gh git jq mkdir mktemp mv realpath rm; do
  command -v "$command" >/dev/null 2>&1 || fail "required command is unavailable: $command"
done
if [[ "$maestro_bin" == */* ]]; then
  [[ -x "$maestro_bin" ]] || fail "OKP_WORKER_FLOOR_MAESTRO_BIN is not executable"
else
  command -v "$maestro_bin" >/dev/null 2>&1 || fail "Maestro command is unavailable: $maestro_bin"
fi

source_repository="$(realpath -e -- "$source_repository")" || fail "source repository does not exist"
source_top="$(git -C "$source_repository" rev-parse --show-toplevel 2>/dev/null)" || \
  fail "OKP_WORKER_FLOOR_SOURCE_REPOSITORY is not a Git worktree"
source_top="$(realpath -e -- "$source_top")" || fail "source repository root does not exist"
[[ "$source_top" == "$source_repository" ]] || \
  fail "OKP_WORKER_FLOOR_SOURCE_REPOSITORY must name the worktree root"

state_dir="$(realpath -m -- "$state_dir")"
if [[ -z "$quarantine_dir" ]]; then
  quarantine_dir="$state_dir/quarantine"
fi
quarantine_dir="$(realpath -m -- "$quarantine_dir")"
case "$state_dir/" in
  "$source_repository/"*) fail "OKP_WORKER_FLOOR_STATE_DIR must be outside the source repository" ;;
esac
case "$quarantine_dir/" in
  "$source_repository/"*) fail "OKP_WORKER_FLOOR_QUARANTINE_DIR must be outside the source repository" ;;
esac

mkdir -p -- "$state_dir"
exec 9>"$state_dir/lock"
if ! flock -n 9; then
  note "another watchdog invocation holds the lock"
  exit 0
fi

fleet_file="$(mktemp "$state_dir/fleet.XXXXXX")"
project_file="$(mktemp "$state_dir/project.XXXXXX")"
issues_file="$(mktemp "$state_dir/issues.XXXXXX")"
issue_file="$(mktemp "$state_dir/issue.XXXXXX")"
pr_file="$(mktemp "$state_dir/pr.XXXXXX")"
cleanup() {
  rm -f -- "$fleet_file" "$project_file" "$issues_file" "$issue_file" "$pr_file"
}
trap cleanup EXIT

declare -a qa_holds=()
normalized_holds="${qa_hold_issues//,/ }"
if [[ -n "${normalized_holds//[[:space:]]/}" ]]; then
  read -r -a qa_holds <<<"$normalized_holds"
fi
declare -A active_qa_holds=()

for issue_number in "${qa_holds[@]}"; do
  is_positive_integer "$issue_number" || \
    fail "OKP_WORKER_FLOOR_QA_HOLD_ISSUES contains a non-positive issue number"

  if ! gh --repo "$repository" issue view "$issue_number" --json state,labels >"$issue_file"; then
    fail "could not inspect QA-hold issue #$issue_number"
  fi
  if ! jq -e '
      type == "object"
      and (.state | type == "string")
      and (.labels | type == "array")
      and all(.labels[]; type == "object" and (.name | type == "string"))
    ' "$issue_file" >/dev/null; then
    fail "QA-hold issue #$issue_number returned malformed state or labels"
  fi

  issue_state="$(jq -r '.state' "$issue_file")"
  if [[ "$issue_state" != "OPEN" ]]; then
    continue
  fi
  active_qa_holds["$issue_number"]=1
  if jq -e --arg ready "$ready_label" 'any(.labels[]; .name == $ready)' \
      "$issue_file" >/dev/null; then
    if ! gh --repo "$repository" issue edit "$issue_number" --remove-label "$ready_label" >/dev/null; then
      fail "could not remove $ready_label from QA-hold issue #$issue_number"
    fi
    note "removed $ready_label from active QA-hold issue #$issue_number"
  fi
done

declare -A claimed_issues=()
live_workers=0
health_state="unknown"
project_paused=true

refresh_project_snapshot() {
  if ! curl --fail --silent --show-error --max-time "$fetch_timeout" \
      --output "$fleet_file" "$fleet_url"; then
    fail "could not fetch the Maestro fleet snapshot"
  fi
  if ! jq -ce --arg project "$project" '
      def nonnegative_integer:
        type == "number" and . >= 0 and . == floor;
      def positive_integer:
        type == "number" and . > 0 and . == floor;
      def valid_claim:
        (type == "number" and positive_integer)
        or (type == "object" and ((.issue_number // .issue) | positive_integer));

      if type != "object" or (.projects | type) != "array" then
        error("fleet response must contain a projects array")
      else
        [.projects[] | select(.name == $project)] as $matches
        | if ($matches | length) != 1 then
            error("fleet response must contain exactly one requested project")
          else
            $matches[0]
          end
      end
      | if (.live_workers | nonnegative_integer)
          and (.paused | type == "boolean")
          and (.outcome.health_state | type == "string")
          and ((.issue_claims == null)
            or ((.issue_claims | type) == "array" and all(.issue_claims[]; valid_claim)))
        then .
        else error("project snapshot has invalid health, pause, worker, or claim evidence")
        end
    ' "$fleet_file" >"$project_file"; then
    fail "Maestro fleet snapshot is not decision-complete"
  fi

  live_workers="$(jq -r '.live_workers' "$project_file")"
  health_state="$(jq -r '.outcome.health_state' "$project_file")"
  project_paused="$(jq -r '.paused' "$project_file")"
  claimed_issues=()
  while IFS= read -r claimed_issue; do
    [[ -n "$claimed_issue" ]] || continue
    claimed_issues["$claimed_issue"]=1
  done < <(jq -r '
      (.issue_claims // [])[]
      | if type == "number" then . else (.issue_number // .issue) end
      | tostring
    ' "$project_file")
}

reconciled_merged_claims=0

reconcile_merged_issue_claims() {
  local issue_number pr_number session issue_state stop_output

  if ! jq -e '
      def positive_integer:
        type == "number" and . > 0 and . == floor;
      def reconciliation_candidate:
        type == "object"
        and (.issue_number | positive_integer)
        and (.pr_number | positive_integer)
        and (.session | type == "string" and length > 0)
        and ((.kind == null) or (.kind | type == "string"))
        and ((.status == null) or (.status | type == "string"));

      [(.issue_claims // [])[] | select(reconciliation_candidate)]
      | group_by(.session)
      | all(.[]; ([.[] | [.issue_number, .pr_number]] | unique | length) == 1)
    ' "$project_file" >/dev/null; then
    fail "project snapshot contains conflicting claims for one Maestro session"
  fi

  while IFS=$'\t' read -r issue_number pr_number session; do
    [[ -n "$issue_number" && -n "$pr_number" && -n "$session" ]] || continue

    if ! gh --repo "$repository" pr view "$pr_number" \
        --json state,mergedAt,body,closingIssuesReferences >"$pr_file"; then
      fail "could not inspect PR #$pr_number for issue #$issue_number"
    fi
    if ! jq -e '
        def positive_integer:
          type == "number" and . > 0 and . == floor;
        type == "object"
        and (.state | type == "string")
        and ((.mergedAt == null) or (.mergedAt | type == "string"))
        and (.body | type == "string")
        and (.closingIssuesReferences | type == "array")
        and all(.closingIssuesReferences[];
          type == "object"
          and (.number | positive_integer)
          and (.repository | type == "object")
          and (.repository.name | type == "string" and length > 0)
          and (.repository.owner | type == "object")
          and (.repository.owner.login | type == "string" and length > 0))
      ' "$pr_file" >/dev/null; then
      fail "PR #$pr_number returned malformed merge evidence"
    fi
    if ! jq -e '.state == "MERGED" and (.mergedAt | type == "string" and length > 0)' \
        "$pr_file" >/dev/null; then
      continue
    fi
    if ! jq -e --arg repository "$repository" --argjson issue "$issue_number" '
        def normalized_lines:
          .body
          | split("\n")
          | map(gsub("^\\s+|\\s+$"; "") | ascii_downcase);
        def normalized_repository:
          split("/")
          | if length >= 2 then .[-2:] | join("/") else . end
          | ascii_downcase;
        ($issue | tostring) as $number
        | ($repository | normalized_repository) as $target_repository
        | any(.closingIssuesReferences[];
            .number == $issue
            and (((.repository.owner.login + "/" + .repository.name) | ascii_downcase)
              == $target_repository))
          or (normalized_lines | any(.[];
            . == ("refs #" + $number)
            or . == ("fixes #" + $number)
            or . == ("closes #" + $number)
            or . == ("resolves #" + $number)))
      ' "$pr_file" >/dev/null; then
      fail "PR #$pr_number does not link claimed issue #$issue_number"
    fi

    if ! gh --repo "$repository" issue view "$issue_number" --json state >"$issue_file"; then
      fail "could not inspect merged issue #$issue_number"
    fi
    if ! jq -e 'type == "object" and (.state | type == "string")' \
        "$issue_file" >/dev/null; then
      fail "issue #$issue_number returned malformed state"
    fi
    issue_state="$(jq -r '.state' "$issue_file")"
    if [[ "$issue_state" == "OPEN" ]]; then
      if ! gh --repo "$repository" issue close "$issue_number" --reason completed >/dev/null; then
        fail "could not close issue #$issue_number after PR #$pr_number merged"
      fi
      note "closed issue #$issue_number after PR #$pr_number merged"
    else
      note "issue #$issue_number is already closed after PR #$pr_number merged"
    fi

    if stop_output="$("$maestro_bin" stop --config "$maestro_config" --session "$session" 2>&1)"; then
      note "stopped merged ghost session $session for issue #$issue_number"
    elif [[ "$stop_output" == *"session $session not found"* ]]; then
      note "merged ghost session $session for issue #$issue_number is already absent"
    else
      fail "could not stop merged ghost session $session for issue #$issue_number"
    fi
    (( reconciled_merged_claims += 1 ))
  done < <(jq -r '
      (.issue_claims // [])
      | map(select(
          type == "object"
          and (.issue_number | type == "number" and . > 0 and . == floor)
          and (.pr_number | type == "number" and . > 0 and . == floor)
          and (.session | type == "string" and length > 0)
          and ((.kind == null) or (.kind | type == "string"))
          and ((.status == null) or (.status | type == "string"))))
      | unique_by([.issue_number, .pr_number, .session])
      | sort_by(.issue_number, .pr_number, .session)
      | .[]
      | [.issue_number, .pr_number, .session]
      | @tsv
    ' "$project_file")
}

gate_allows_spawn() {
  if [[ "$project_paused" == "true" ]]; then
    note "project is paused; no spawn"
    return 1
  fi
  if [[ "$health_state" != "healthy" ]]; then
    note "project outcome is $health_state; no spawn"
    return 1
  fi
  if (( live_workers > 0 )); then
    note "project already has $live_workers live worker(s)"
    return 1
  fi
  return 0
}

refresh_project_snapshot
reconcile_merged_issue_claims
if (( reconciled_merged_claims > 0 )); then
  refresh_project_snapshot
fi
gate_allows_spawn || exit 0

if ! gh --repo "$repository" issue list \
    --state open \
    --label "$ready_label" \
    --limit "$issue_limit" \
    --json number,labels,createdAt >"$issues_file"; then
  fail "could not list ready issues"
fi
if ! jq -e '
    def positive_integer:
      type == "number" and . > 0 and . == floor;
    type == "array"
    and all(.[];
      type == "object"
      and (.number | positive_integer)
      and (.createdAt | type == "string" and length > 0)
      and (.labels | type == "array")
      and all(.labels[]; type == "object" and (.name | type == "string")))
  ' "$issues_file" >/dev/null; then
  fail "ready issue query returned malformed queue evidence"
fi

mapfile -t candidates < <(jq -r --arg blocked "$blocked_label" '
    sort_by(.createdAt, .number)
    | .[]
    | select((any(.labels[]; .name == $blocked)) | not)
    | .number
  ' "$issues_file")

selected_issue=""
for issue_number in "${candidates[@]}"; do
  if [[ -n "${active_qa_holds[$issue_number]+held}" ]]; then
    note "issue #$issue_number is on active QA hold"
    continue
  fi
  if [[ -n "${claimed_issues[$issue_number]+claimed}" ]]; then
    note "issue #$issue_number already has a Maestro claim"
    continue
  fi
  selected_issue="$issue_number"
  break
done

if [[ -z "$selected_issue" ]]; then
  note "no eligible ready issue"
  exit 0
fi

if ! gh --repo "$repository" issue view "$selected_issue" --json state,labels >"$issue_file"; then
  fail "could not recheck selected issue #$selected_issue"
fi
if ! jq -e '
    type == "object"
    and (.state | type == "string")
    and (.labels | type == "array")
    and all(.labels[]; type == "object" and (.name | type == "string"))
  ' "$issue_file" >/dev/null; then
  fail "selected issue #$selected_issue returned malformed state or labels"
fi
if ! jq -e --arg ready "$ready_label" --arg blocked "$blocked_label" '
    .state == "OPEN"
    and any(.labels[]; .name == $ready)
    and ((any(.labels[]; .name == $blocked)) | not)
  ' "$issue_file" >/dev/null; then
  note "selected issue #$selected_issue is no longer eligible"
  exit 0
fi

# Refresh immediately before touching the canonical checkout. The spawn command
# remains the final authority if another scheduler wins the remaining race.
refresh_project_snapshot
gate_allows_spawn || exit 0
if [[ -n "${claimed_issues[$selected_issue]+claimed}" ]]; then
  note "selected issue #$selected_issue acquired a Maestro claim"
  exit 0
fi

current_branch="$(git -C "$source_repository" symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
[[ "$current_branch" == "$expected_branch" ]] || \
  fail "canonical source checkout is not on the configured branch"

declare -a configured_quarantine_roots=()
normalized_roots="${quarantine_roots//,/ }"
if [[ -n "${normalized_roots//[[:space:]]/}" ]]; then
  read -r -a configured_quarantine_roots <<<"$normalized_roots"
fi

quarantine_stamp="$(date -u +%Y%m%dT%H%M%SZ)"
for relative_root in "${configured_quarantine_roots[@]}"; do
  [[ "$relative_root" =~ ^[A-Za-z0-9._-]+$ ]] || \
    fail "OKP_WORKER_FLOOR_QUARANTINE_ROOTS must contain top-level relative names"
  [[ "$relative_root" != "." && "$relative_root" != ".." ]] || \
    fail "OKP_WORKER_FLOOR_QUARANTINE_ROOTS contains an unsafe entry"

  root_status="$(git -C "$source_repository" status --porcelain=v1 --untracked-files=all -- "$relative_root")"
  [[ -n "$root_status" ]] || continue
  if [[ -n "$(git -C "$source_repository" ls-files -- "$relative_root")" ]]; then
    fail "refusing to quarantine a root containing tracked files"
  fi
  [[ -e "$source_repository/$relative_root" || -L "$source_repository/$relative_root" ]] || \
    fail "agent-junk path changed while quarantine was being prepared"

  mkdir -p -- "$quarantine_dir"
  quarantine_name="${relative_root#.}"
  [[ -n "$quarantine_name" ]] || quarantine_name="agent-junk"
  destination="$quarantine_dir/$quarantine_name-$quarantine_stamp-$$"
  collision=0
  while [[ -e "$destination" || -L "$destination" ]]; do
    (( collision += 1 ))
    destination="$quarantine_dir/$quarantine_name-$quarantine_stamp-$$-$collision"
  done
  if ! mv -- "$source_repository/$relative_root" "$destination"; then
    fail "could not quarantine agent-junk root $relative_root"
  fi
  note "quarantined agent-junk root $relative_root"
done

if [[ -n "$(git -C "$source_repository" status --porcelain=v1 --untracked-files=all)" ]]; then
  fail "canonical source checkout remains dirty after allowlisted quarantine; no spawn"
fi

note "spawning oldest eligible issue #$selected_issue"
if ! "$maestro_bin" spawn --config "$maestro_config" --issue "$selected_issue"; then
  echo "worker-floor: Maestro spawn failed for issue #$selected_issue" >&2
  exit 1
fi
note "spawned issue #$selected_issue"
