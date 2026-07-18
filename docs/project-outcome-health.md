# Project outcome health

`scripts/check-project-outcome.sh` is the versioned, read-only project outcome
checker. It replaces host-local delivery heuristics with a repository contract
implemented by `okp_core::project_health` and the `okp-candidate project-health`
CLI command.

Run it from a current checkout:

```bash
scripts/check-project-outcome.sh
```

The checker emits one JSON outcome and uses these exit codes:

- `0`: every blocking development-health check passed;
- `1`: a blocking check failed with its reason in `checks[].details`;
- `2`: the checker itself could not start or its snapshot was invalid.

It is strictly observational. It does not dispatch a workflow, upload or
rewrite a feed, publish a candidate, create a release, or change repository or
orchestration settings.

## Blocking development contract

The overall result is healthy only when all three blocking surfaces pass:

1. The latest `main` SHA is exact, and the `CI` and `Rust` push workflows for
   that same SHA both completed successfully.
2. The Windows static feed is reachable JSON with at least one Full package;
   every asset has a version/package identity, a non-zero size, an absolute
   HTTPS URL, and a SHA-256 identity.
3. The rolling Linux `candidate.linux.json` is reachable and valid. It must
   declare `channel=candidate` and `acceptance=accepted`; carry a non-zero build
   encoded by its monotonic candidate version; carry an exact 40-character
   source SHA; name the exact Debian and Velopack Full package URLs, sizes, and
   SHA-256 identities plus the versioned checksum URL; and contain a strict
   descending history when history is present.

The Linux Candidate workflow itself must report GitHub state `active`. Any
other state is blocking and emits the machine-readable reason code
`candidate-workflow-inactive` while naming the observed state in the detail.
When the accepted candidate is behind `main`, the newest completed `schedule`
run must also be fresh. The default freshness window is **45 minutes**, three
times the 15-minute cron interval. No completed schedule run inside that window
emits `candidate-schedule-stale`, independently of candidate publication lag.

Candidate and commit timestamps are strict UTC (`YYYY-MM-DDTHH:MM:SSZ`) and may
not be more than five minutes in the future. The candidate timestamp must not
predate its source commit. Timestamp age is evidence, not the delivery clock:

- When the accepted candidate source equals current `main`, development
  delivery is current indefinitely. Wall-clock age alone does not fail health,
  and an unchanged SHA is not rebuilt.
- When the candidate source is an ancestor behind current `main`, the clock
  starts at the first unpublished `main` commit after that source. The default
  limit is **120 minutes**: the documented 60–90 minute delivery SLA plus one
  bounded 30-minute scheduler/publication grace window.
- When the candidate source is not an ancestor of current `main`, health fails
  even if the candidate feed timestamp is recent.

The collector records the candidate commit timestamp, the candidate-to-main
relation, and the first unpublished main commit SHA and timestamp. This commit
graph evidence makes snapshot evaluation deterministic. Operators may
temporarily select another positive lag bound with
`OKP_PROJECT_HEALTH_MAX_UNPUBLISHED_MAIN_LAG_SECONDS`, but the versioned default
is the project contract.

Operators may separately set a positive schedule window with
`OKP_PROJECT_HEALTH_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS`. Workflow state and the
latest completed schedule run are collected with bounded list/API queries; the
workflow-state API response is cached for five minutes. Schedule freshness is
not blocking while the accepted candidate already equals `main`, because no
new delivery is pending.

An unreachable, malformed, pending/rejected, partial, identity-incomplete,
source-divergent, or over-SLA candidate fails with a specific reason. The
checker only reads the rolling pointer; it never triggers a duplicate candidate
publication.

## Stable-release diagnostic

The newest permanent `linux-v*` release is still reported. Once it is older
than 48 hours its check becomes `warning`, but it is explicitly
`blocking: false`. Permanent public release cadence and rolling development delivery are
different signals: an old public release cannot mark an accepted, source-current
QA candidate dead or block PR repair/merge work.

## Deterministic snapshots

The collector and evaluator are separable. A captured or test snapshot can be
evaluated without network access:

```bash
scripts/check-project-outcome.sh --snapshot project-health-snapshot.json
```

The repository includes a complete healthy replay fixture:

```bash
scripts/check-project-outcome.sh --snapshot \
  rust/crates/okp-core/tests/fixtures/project_health/fresh-accepted-snapshot.json
```

Snapshot mode is offline by construction: it never invokes Cargo or any remote
command. Both modes use `OKP_PROJECT_HEALTH_BIN` when set, otherwise an
already-built `rust/target/release/okp-candidate` or
`rust/target/debug/okp-candidate`. Neither mode invokes Cargo: this keeps the
live healthcheck inside its bounded runtime. If no local evaluator is
executable, the command exits `2` with instructions to build it outside the
healthcheck.

`okp-core` fixtures cover old accepted/equal, ancestor within SLA, ancestor
beyond SLA, inactive workflow state, stale completed schedules while `main` has
advanced, fresh non-ancestor, unaccepted, malformed, missing-package-identity,
and unreachable candidate outcomes. The public release warning is separately
pinned as non-blocking.

## Safe operational cutover after merge

1. Update the automation checkout to the merged commit; invoke the checker from
   that checkout instead of copying it to an unversioned host-local location.
2. For at least one normal candidate interval, run the old and new checks in
   observation-only mode. Confirm that source/main CI and the Windows feed agree.
   Confirm separately that an equal accepted source stays healthy regardless of
   age, an ancestor source uses the first unpublished-commit clock, and an old
   `linux-v*` release is only a warning.
3. Switch only the project-outcome command to
   `scripts/check-project-outcome.sh`. Do not change the candidate schedule,
   workflow concurrency, worker eligibility, publication, or global
   orchestration settings as part of the cutover.
4. Keep the prior command available for rollback until the versioned checker
   has completed multiple normal intervals. A checker startup error is exit 2
   and should be repaired or rolled back; it must not be converted into a
   healthy result.

The repository change deliberately does not perform this external cutover. It
provides the merged, reviewable command that the operator can pin safely.
