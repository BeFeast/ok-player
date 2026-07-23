# Linux night GUI QA driver

The repository owns the public, reviewable contract for the lease-gated Linux
night GUI suite. Site-local timers, SSH configuration, credentials, desktop
automation, and artifact publication remain outside the repository.

Run the controller during its scheduled UTC `00:05` through `05:05` window:

```bash
scripts/ok-player-night-gui-qa.sh
```

The default automatic order is `slava`, then `mimir`, then `baldr` when each
host is reachable and its lease is free. Set `OKP_QA_HOSTS` to a
whitespace-separated list of sanitized logical aliases when a deployment uses
a different eligible set or order. Whitespace may include line breaks, and
aliases are normalized with locale-independent ASCII lowercase before duplicate
and protected-host checks:

```bash
OKP_QA_HOSTS='mimir baldr' scripts/ok-player-night-gui-qa.sh
```

The controller visits every eligible host in the configured order and rejects
empty, invalid, duplicate, or `sindri` entries. Alias identity and the protected
role check use locale-independent ASCII case folding, matching SSH alias
behavior even when the timer inherits a non-English locale. Do not put physical
hostnames or private addresses in this variable because the logical alias is
copied into the evidence metadata. `sindri` is never in the automatic list. A
one-host operator-authorized run requires all three explicit choices:

```bash
OKP_QA_ALLOW_SINDRI=1 OKP_QA_OPERATOR_GO=1 \
  scripts/ok-player-night-gui-qa.sh --host sindri --force-window
```

Do not set those variables without a direct operator go. A timer must never set
them. `--force-window` is for an attended daytime run; it does not weaken the
lease or operator-seat guard.

## Exclusive lease

`scripts/ok-player-qa-lease.sh` manages `~/qa/LEASE`. It uses `flock` plus an
atomic replacement, records the logical host role, suite ID, owner PID, and UTC
acquisition/expiry times, and defaults to a 45-minute TTL. A valid lease owned
by another suite returns exit `2`. An expired or malformed lease may be
replaced while holding the lock. Release requires the exact suite ID.

The controller bounds each host runner to 30 seconds less than its lease TTL
and terminates the runner's process group if it overruns. This prevents input
automation from continuing after the exclusive lease becomes reclaimable.

The controller performs only a read-only host reachability probe before
acquiring the lease. Candidate preparation and the headless Xvfb regression run
inside the host runner after the lease succeeds. The runner then checks the
graphical seat before any real desktop launch, input injection, screenshot, or
live Wave A/B/C action, so an occupied or unavailable seat cannot receive
automation. A no-seat host still retains the headless regression result and
records the live rows as `NOT RUN`. The runner stops before every action when
accepted-candidate preparation is missing or fails. On exit or interruption the
controller releases only its own suite lease; site hooks must likewise kill
only processes whose identities they recorded for that suite.

## Host hook contract

The controller streams the versioned lease and host scripts over SSH; it does
not install repository files or assume a checkout on a QA laptop. Each host
provides executable hooks under
`~/.local/lib/ok-player-night-gui-qa/hooks/` (or `OKP_QA_HOOK_DIR`):

| Hook | Arguments | Responsibility |
| --- | --- | --- |
| `probe-seat` | host role | Require active, unlocked graphical `seat0` with no operator conflict. |
| `prepare-candidate` | artifact directory, host role, suite ID | Select and install the accepted rolling candidate, and write the required `candidate.env` identity. |
| `probe-dual-head` | artifact directory, host role, suite ID | Exit `0` only when two active heads are available; exit `1` for a proved single-head state; exit `75` when unknown. |
| `run-action` | action, artifact directory, host role, suite ID | Perform one named action and retain complete sanitized evidence. `headless_window_regressions` must not use the real desktop seat. |

Hooks return `0` for `PASS`, `75` for `NOT RUN`, and another non-zero status for
`FAIL`. A missing hook is `NOT RUN`; the host run exits `4` rather than claiming
acceptance. Inapplicable Wave B rows on a proved single-head host and Wave C
rows on other roles are recorded as `SKIP`, not as missing evidence. Site hooks
must not print credentials, private addresses, account
names, machine paths, or physical hostnames into evidence intended for public
review.

`prepare-candidate` cannot pass on exit status alone. It must write
`candidate.env` with `acceptance=accepted`, a 40-character lowercase
`source_sha`, `version`, package filename, 64-character lowercase
`package_sha256`, and 64-character lowercase `manifest_sha256`. The runner
validates those fields before it permits the first GUI action.

The actions are:

- Wave A on every eligible host: candidate install, the headless window
  regression harness, cold launch, single-monitor fit, play/pause/seek, ten
  non-OSC surface drags with process survival and observed window movement,
  menus/settings/chapters, secondary launch, and clean close.
- Wave B only after a passing dual-head probe: open from each head, prove the
  fitted window does not span heads, and drag near a workarea edge.
- Wave C only on the weak-host role (`slava`): 4K stress, rapid open/close, and
  a seek/screenshot storm.

For the `headless_window_regressions` action, the night suite's `run-action`
hook invokes the aggregate helper and retains its output inside the suite
artifact directory. Build or select the exact candidate binary and provide the
candidate source revision explicitly:

```bash
OKP_WINDOW_REGRESSION_SOURCE_SHA=<candidate-source-sha> \
  scripts/run-linux-window-regression-smokes.sh \
  <candidate-binary> <artifact-directory>/window-regressions
```

The helper always attempts both regressions, writes `results.tsv` and
`summary.env`, and binds the key drag, fit, Xvfb, and D-Bus evidence files in
`SHA256SUMS`. The output directory must not already exist. The helper reads the
source revision from Git when available; an exported tree must provide an exact
lowercase 40-character commit through `OKP_WINDOW_REGRESSION_SOURCE_SHA`. It
also rejects fit evidence that names a different revision, incomplete drag
assertions, an incomplete three-run fit series, and missing or unsuccessful
Xvfb/D-Bus evidence. The headless action requires no operator seat and is
suitable for CI or an unattended night host with the existing smoke
dependencies installed. Its Xvfb/X11 results cannot replace the live
GNOME/Wayland pointer, compositor, focus, portal, or dual-head rows; the live
`single_monitor_fit` and `non_osc_drag_10` actions still require actual desktop
observations. CI runs the aggregate helper's dispatch/failure/evidence policy
test, while the Rust suite also pins the required drag and fit assertions in
the underlying scripts.

## Artifacts and timer ownership

Every run stays below:

```text
~/qa/okp-night-YYYYMMDD/<host>/runs/<suite-id>/
```

`results.tsv` contains one Wave/action result, `metadata.env` contains sanitized
logical run identity, and `SHA256SUMS` binds every top-level evidence file. The
host directory also records the latest suite ID. Large logs, screenshots, and
packages stay out of git; an issue-owned acceptance decision must publish them
to durable storage and add the required `docs/qa-records/` Markdown record.

Maestro already owns `ok-player-night-gui-qa.timer` at UTC `00:05`, `01:05`,
`02:05`, `03:05`, `04:05`, and `05:05` (approximately Israel `03:05` through
`08:05` during daylight time). This repository does not install, rewrite, or
enable that timer. Deployment should point the existing service at this
controller and provide only the site-local hooks and SSH policy.
