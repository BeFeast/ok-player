# APT repository lane

Debian and Ubuntu users should not have to watch a Releases page. This lane publishes a signed
APT repository on the same GitHub Pages site that already serves the JSON update feeds, so
`apt upgrade` — and every desktop update notifier built on top of it — carries OK Player along
with the rest of the system.

Served at `https://befeast.github.io/ok-player/apt/`:

```
apt/ok-player-archive-keyring.asc          armored public signing key
apt/ok-player-archive-keyring.gpg          the same key dearmored, for /usr/share/keyrings
apt/ok-player.sources                      a ready-made deb822 source stanza
apt/pool/main/o/ok-player/*.deb            the packages
apt/dists/stable/Release                   the archive index
apt/dists/stable/InRelease                 inline-signed Release
apt/dists/stable/Release.gpg               detached signature over Release
apt/dists/stable/main/binary-amd64/{Packages,Packages.gz,Release}
```

User-facing instructions live in the [README](../README.md#install-on-linux).

## Derived, never authored

`scripts/build-apt-repo.sh` is a pure function of the published `linux-v*` GitHub releases plus
the signing key, exactly like `scripts/build-win-feed.sh` and `scripts/build-linux-feed.sh`. It
never reads the previously deployed site, so there is no state to drift: `actions/deploy-pages`
replaces the whole site in one step, and the archive it replaces it with is reproduced from the
releases rather than mutated in place.

Three properties follow, and all three are pinned by `scripts/tests/apt-repo-generator.Tests.sh`:

* **Additive.** Publishing a version only ever adds a pool file and a `Packages` paragraph. The
  pool bytes come from an immutable release asset, so a version already published keeps its
  exact `Size` and `SHA256`. A client that is mid-download, or that pinned a version, is never
  handed a changed checksum.
* **Idempotent.** A rerun over an unchanged release set reproduces the archive content byte for
  byte. `Packages` is ordered by `dpkg-scanpackages`' stable version order, `Packages.gz` is
  written with `gzip -n` so no timestamp leaks into it, and `Release`'s `Date` is taken from the
  newest retained release's publication time rather than from the clock. This matters because
  the workflow also runs on a docs push and on every Windows release: those runs must not
  rewrite what apt clients already have.

  The two OpenPGP signatures are deliberately *not* byte-stable. Every signature carries its own
  creation time, and back-dating one to an old release just to make bytes repeat would be a lie
  about when it was made. The tests compare everything except `InRelease` and `Release.gpg`, and
  separately assert that both verify against the published public key.
* **Bounded.** GitHub Pages publishes roughly 1 GB per site and a Linux package that bundles
  libmpv is ~76 MB, so the archive is a rolling window: the newest `OKP_APT_MAX_VERSIONS`
  (default 10) releases that also fit inside `OKP_APT_POOL_BUDGET_BYTES` (default 900 MiB).
  Versions outside the window drop out of the pool and the index *together*, so the archive
  never carries a dangling reference, and they remain downloadable from their own GitHub
  release — which is what the standalone `.deb` self-updater already uses. The window is the one
  place where "additive" is bounded by physics; everything inside it is strictly additive.

  The **current release is not optional**. If it cannot fit — because the budget is too small or
  the version count is — the lane aborts rather than trimming it and keeping its predecessors.
  A signed archive advertising an older version than the JSON feeds do is worse than no publish
  at all, because apt clients would accept it.

## Signing

The archive signing key never becomes a GitHub Actions secret and never lands in an artifact.
`build-apt-repo.sh` fetches it from Infisical at run time and handles it under these rules:

* Non-secret coordinates live in the script: host `secrets.oklabs.uk`, project `services`,
  environment `prod`, secret path `/ok-player`. Only the Universal Auth credentials
  (`INFISICAL_CLIENT_ID` / `INFISICAL_CLIENT_SECRET`) come from GitHub Actions secrets. The
  identity behind them is read-only and scoped to this path, so a compromised runner cannot
  write secrets or reach another project.
* Secrets read at run time: `gpg-private-key`, `gpg-public-key`, `gpg-fingerprint`,
  `gpg-passphrase`.
* The private key is imported into an ephemeral `GNUPGHOME` created under the runner temp
  directory, which is destroyed — `gpgconf --kill all` then `rm -rf` — on exit, including on
  failure and on a signal.
* **The imported key's fingerprint is compared against `gpg-fingerprint` before anything is
  signed**, and a mismatch aborts. Publishing an archive signed by an unexpected key is worse
  than publishing nothing: apt clients that already trust this archive would accept it.
* Signing is non-interactive with `--batch --pinentry-mode loopback --passphrase-fd 3`, and the
  passphrase reaches fd 3 from a bash here-string, so it never appears in a command line or in
  the process table of a shared runner. The Infisical access token is passed the same way,
  through a `curl --config` stanza on stdin rather than an `-H` argument.
* Fetched values are masked in the Actions log line by line, and nothing in this script lists
  secret keys.
* A missing or unreadable secret aborts the lane naming the secret (for example
  `services/prod/ok-player:gpg-passphrase`). There is no unsigned fallback.
* Only the public key is published — as both `.asc` and a dearmored `.gpg`.

Because the whole site deploys atomically, a failure here blocks the JSON feeds too. That is the
intended trade: a broken or unsigned APT archive is a worse outcome than a delayed feed refresh,
and the refresh is idempotent and re-runnable from the *Update Feeds* workflow.

### Why there is no `Valid-Until`

`Release` deliberately carries no `Valid-Until`. The field bounds a freeze or rollback attack —
an attacker able to intercept the transport could otherwise keep serving an old, signed index
forever. But apt rejects an archive outright once the field lapses, and OK Player releases on no
schedule: any window short enough to be worth having is short enough to strand every user
during a quiet month, turning a security nicety into an outage. The archive is served over HTTPS
from GitHub Pages and regenerated on every deploy. Revisit this if the release cadence ever
becomes predictable.

## Verification

`scripts/verify-apt-repo.sh` runs apt against the generated archive in a throwaway
`debian:13-slim` container — the archive is bind-mounted and consumed through a `file://` source,
so nothing has to be deployed first. It adds the published keyring and source, runs `apt-get
update` and `apt-get install ok-player`, then starts `/usr/bin/ok-player` headless and requires
it to reach its GUI initialisation (the process then dies on the absent display, which is
expected — getting that far is the "the packaged binary and its dependency closure are real"
signal used by the other Linux gates).

Which signal is required is derived from the packaged binary rather than fixed. Current builds
log `Renderer policy:` as the first statement of `main()`, before GTK, so for them that line is
mandatory. Releases published before that line existed — the newest published `linux-v*` release
is one — can only prove they reached GTK's display connection, and the archive has to stay
verifiable over the versions it actually carries. An unresolved shared library is a hard failure
either way, which is what this check exists to catch.

It then runs two negative controls. Both check *two* things, because by default `apt-get update`
reports a rejected repository as a warning, keeps the previously fetched index and still exits 0
— asserting on its exit status alone would pass even if apt had happily used a forged index. So
each control asserts that the update fails under `APT::Update::Error-Mode=any` **and** that, with
every cached index discarded first, the package is genuinely not installable:

* a tampered `InRelease` is refused;
* an archive whose signature does not match the keyring is refused.

Given a second, older archive it additionally proves the upgrade path: install from the old
archive, publish the new one over the same repository, and require `apt-get upgrade` to move to
the newer version while the older one stays installable by explicit version.

"Newest" is derived with `dpkg --compare-versions`, not by taking the last paragraph of the
index. `dpkg-scanpackages` keys its output by package name and version and emits it in
lexicographic key order — `--multiversion` only allows several versions of one package, it does
not sort them — so `0.11.0-beta.10` lands before `0.11.0-beta.9`. apt is unaffected: it reads
every paragraph and picks the maximum. Only an expectation built on paragraph order would be
wrong, and it would fail the whole lane the first time a version crossed a decimal boundary.

A missing container runtime is a hard failure (exit 127), not a skip. This gate is the only thing
that distinguishes a working archive from a plausible-looking one.

## Operating it

* The lane runs inside *Update Feeds* (`.github/workflows/publish-update-feeds.yml`), which is
  the only writer of the Pages site. `release-linux.yml` calls it with `secrets: inherit` after
  publishing a `linux-v*` release; a manual `workflow_dispatch` re-runs it at any time.
* Rotating the signing key means updating the four Infisical secrets and letting the next run
  republish `ok-player-archive-keyring.{asc,gpg}`. Existing installs need the new key before the
  first archive signed with it is served, so publish a run with the *old* key still in place
  after users have fetched the new keyring, or accept that clients must re-fetch it.
* `scripts/tests/apt-repo-generator.Tests.sh` runs in the *Rust* workflow next to the other
  script-level policy suites. It needs no network, no Infisical and no container: it builds real
  `.deb` files with `dpkg-deb` and drives the real generator with a throwaway key.
