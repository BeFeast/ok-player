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

The overall result is healthy only when all four blocking surfaces pass:

1. The latest `main` SHA is exact, and the `CI` and `Rust` push workflows for
   that same SHA both completed successfully. A newly pushed SHA gets a bounded
   15-minute settling window while matching runs are queued/in progress or have
   not appeared in the run index yet. That state is a blocking `warning`, which
   keeps the overall outcome healthy; a completed failure fails immediately,
   and unfinished evidence fails after the window expires.
2. The Windows static feed is reachable JSON with at least one Full package;
   every asset has a version/package identity, a non-zero size, an absolute
   HTTPS URL, and a SHA-256 identity.
3. The rolling Windows `candidate.windows.json` identity manifest and
   `releases.win-candidate.json` feed are reachable and agree byte-for-byte on
   the feed SHA-256 and size. The manifest must carry the `win-candidate`
   channel, monotonic version/build, exact source SHA, UTC promotion timestamp,
   builder identity, and one current manifest-bound Full package for
   `com.befeast.okplayer`.
4. The rolling Linux `candidate.linux.json` is reachable and valid. It must
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

Operators may temporarily select another positive main-CI settling window with
`OKP_PROJECT_HEALTH_SOURCE_CI_GRACE_SECONDS`. The collector binds the window to
the exact main commit timestamp, so an old or indefinitely queued workflow
cannot remain healthy. Missing or mismatched CI/Rust results only warn inside
the window; malformed source evidence and completed workflow failures remain
immediate failures.

Operators may separately set a positive schedule window with
`OKP_PROJECT_HEALTH_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS`. Workflow state and the
latest completed schedule run are collected with bounded list/API queries; the
workflow-state API response is cached for five minutes. Schedule freshness is
not blocking while the accepted candidate already equals `main`, because no
new delivery is pending.

The collector reads up to 100 completed scheduled candidate runs and counts the
failure streak from newest to oldest. At two or more consecutive failures it
reads the newest failed log for the builder's `failed at gate <name>` marker and
emits `candidate builds failing: gate <name> (<N> consecutive)` with reason code
`candidate-builds-failing`. This explicit builder failure is ordered before the
generic unpublished-main lag detail.

An unreachable, malformed, pending/rejected, partial, identity-incomplete,
source-divergent, or over-SLA candidate fails with a specific reason. The
checker only reads the rolling pointer; it never triggers a duplicate candidate
publication.

## Windows candidate delivery

The `windows-candidate-delivery` row uses the same 120-minute unpublished-main
lag bound as Linux. When the promoted Windows source equals `main`, an unchanged
repository is healthy indefinitely. When `main` advances, the clock starts at
the first unpublished commit after the promoted source; the row remains passing
inside the delivery window and fails once that lag exceeds the bound. A source
that is not an ancestor of current `main` is invalid even when its timestamp is
recent.

The collector reads up to 100 completed scheduled `Windows Candidate` runs. Two
or more consecutive failures are reported before generic lag evidence as
`Windows candidate builder failing at gate <name> (<N> consecutive)`. The gate
is the failed workflow step from the newest failed run, and the reason code is
`windows-candidate-builds-failing`. While the new lane has no completed schedule
history and has not published either pointer, the row is a blocking `warning`
rather than a failure; warnings do not make the overall outcome unhealthy.

The live collector starts the stable Windows feed, Windows candidate manifest,
Windows candidate feed, and Linux candidate feed requests concurrently. Each
request retains the existing connection/retry/30-second bound, so adding the
Windows evidence does not serialize another network timeout into the fleet
pulse. Snapshot mode remains fully offline and decision-complete in `okp-core`.

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

`okp-core` fixtures cover bounded main-CI settling, overdue pending CI, immediate
completed CI failures, old accepted/equal, ancestor within SLA, ancestor
beyond SLA, inactive workflow state, stale completed schedules while `main` has
advanced, fresh non-ancestor, unaccepted, malformed, missing-package-identity,
and unreachable Linux candidate outcomes. Windows fixtures separately pin
source-current and within-SLA passes, over-SLA and consecutive-gate failures,
the no-history bootstrap warning, manifest/feed identity, and bounded live
collection. The public release warning is separately pinned as non-blocking.

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
