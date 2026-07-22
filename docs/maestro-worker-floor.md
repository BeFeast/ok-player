# Maestro worker-floor watchdog

`scripts/ok-player-worker-floor.sh` is the versioned policy for keeping one OK
Player implementation worker alive. The host timer remains operator-managed;
the repository does not install a service, choose a fleet address, or contain
private host identities or credential paths.

On each invocation the watchdog:

1. takes a non-blocking project lock;
2. inspects the configured QA-hold issues and removes `ok-player-ready` from
   every hold that is still open;
3. requires exactly one matching fleet project with `paused=false`,
   `outcome.health_state=healthy`, and `live_workers=0`;
4. orders open `ok-player-ready` issues by `createdAt` and issue number, then
   skips `blocked`, active QA-hold, and already claimed issues;
5. rechecks the selected issue and the fleet snapshot;
6. quarantines allowlisted agent-junk roots from the canonical `main` checkout
   and refuses to continue if any other dirty state remains; and
7. calls `maestro spawn` once for the oldest remaining issue.

Missing, duplicate, or malformed evidence fails closed. Numeric Maestro claims
and object claims such as `{"issue_number": 123}` are both supported. The
spawn command remains the final claim authority if another scheduler wins the
small race after the last snapshot.

## Required environment

Keep these values in a private service environment file rather than editing the
script:

| Variable | Contract |
| --- | --- |
| `OKP_WORKER_FLOOR_CONFIG` | Absolute path to the Maestro project configuration used by `maestro spawn`. |
| `OKP_WORKER_FLOOR_FLEET_URL` | Fleet API URL that exposes the project `live_workers`, `paused`, outcome, and issue claims. |
| `OKP_WORKER_FLOOR_SOURCE_REPOSITORY` | Absolute path to the canonical source checkout; it must be the worktree root and be on `main` before a spawn. |
| `OKP_WORKER_FLOOR_STATE_DIR` | Private directory outside the source checkout for the invocation lock and temporary snapshots. |

`OKP_WORKER_FLOOR_QUARANTINE_DIR` defaults to `quarantine/` under the state
directory and must also remain outside the source checkout. The service must
provide any Maestro credentials through its own private environment; the
watchdog neither chooses nor embeds a credentials file.

## Policy overrides

The public defaults are:

- repository `BeFeast/ok-player` and fleet project `ok-player`;
- ready label `ok-player-ready` and blocked label `blocked`;
- QA holds `545 546`, active only while each issue is open;
- canonical branch `main`;
- quarantine roots `.agents .claude .cursor`;
- an eight-second fleet request timeout; and
- at most 1,000 ready issues in one queue read.

Operators can override these with the corresponding
`OKP_WORKER_FLOOR_REPOSITORY`, `OKP_WORKER_FLOOR_PROJECT`,
`OKP_WORKER_FLOOR_READY_LABEL`, `OKP_WORKER_FLOOR_BLOCKED_LABEL`,
`OKP_WORKER_FLOOR_QA_HOLD_ISSUES`, `OKP_WORKER_FLOOR_SOURCE_BRANCH`,
`OKP_WORKER_FLOOR_QUARANTINE_ROOTS`,
`OKP_WORKER_FLOOR_FETCH_TIMEOUT_SECONDS`, and
`OKP_WORKER_FLOOR_ISSUE_LIMIT` variables. Issue and root lists accept spaces or
commas. `OKP_WORKER_FLOOR_MAESTRO_BIN` may select a pinned Maestro executable.

QA-hold cleanup deliberately runs before the health, pause, and worker-count
gates. A project pause stops worker selection and spawning, but it does not
leave a live-acceptance issue advertised as ready for implementation.

## Dirty-checkout quarantine

Quarantine is intentionally narrow. A configured entry must be one top-level
relative name. The watchdog moves it only when Git reports changes below that
root and no tracked file exists there. It never resets, cleans, overwrites, or
deletes source changes. After the moves, any remaining tracked or untracked
change makes the invocation fail without spawning; an operator can then inspect
the canonical checkout and the private quarantine directory.

## Timer wiring

A host can invoke the merged script from an existing checkout with a oneshot
service like this placeholder-only example:

```ini
[Unit]
Description=OK Player worker-floor watchdog

[Service]
Type=oneshot
EnvironmentFile=%h/.config/ok-player/worker-floor.env
ExecStart=/absolute/path/to/checkout/scripts/ok-player-worker-floor.sh
TimeoutStartSec=180
```

```ini
[Unit]
Description=Run the OK Player worker-floor watchdog every ten minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=10min
RandomizedDelaySec=45

[Install]
WantedBy=timers.target
```

The environment file supplies the actual fleet URL and filesystem locations;
they are not suitable for the public unit example. Update the external service
to a merged checkout, run one manual invocation, and then enable or restart the
timer. A successful spawn and a policy no-op both exit `0`; an actuator failure
exits `1`; invalid prerequisites, malformed evidence, or unsafe checkout state
exit `2` so the service remains visibly failed.

The watchdog does not change Greptile or merge policy, project concurrency, or
`max_parallel`. Its only GitHub mutation is removal of the ready label from an
active QA hold, followed by the separately authorized Maestro spawn when every
gate passes.
