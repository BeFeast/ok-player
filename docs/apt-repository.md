# APT repository lane

Debian and Ubuntu users should not have to watch a Releases page. This lane publishes a signed
APT repository on the same GitHub Pages site that already serves the JSON update feeds, so
`apt upgrade` — and every desktop update notifier built on top of it — carries OK Player along
with the rest of the system.

Served at `https://befeast.github.io/ok-player/apt/`:

```
apt/ok-player-archive-keyring.asc          armored public signing key
apt/ok-player-archive-keyring.gpg          the same key dearmored, for /usr/share/keyrings
apt/ok-player.sources                      a ready-made deb822 stanza for the stable suite
apt/ok-player-candidate.sources            the same for the candidate (QA) suite
apt/pool/main/o/ok-player/*.deb            the packages, shared by both suites
apt/dists/<suite>/Release                  the archive index
apt/dists/<suite>/InRelease                inline-signed Release
apt/dists/<suite>/Release.gpg              detached signature over Release
apt/dists/<suite>/main/binary-amd64/{Packages,Packages.gz,Release}
```

User-facing instructions live in the [README](../README.md#install-on-linux).

## Two suites (issue #689)

| Suite | Built from | Who subscribes | Source stanza |
|---|---|---|---|
| `stable` | the published `linux-v*` releases | everyone | `ok-player.sources` |
| `candidate` | the rolling `linux-candidate` release | testers, deliberately | `ok-player-candidate.sources` |

The two exist because they answer different questions. `stable` means "a version that was
released", and it is gated behind the acceptance manifest that versioned Linux releases require.
`candidate` means "the build the QA lane is currently pointing at". Before #689 only `stable` was
published, so when the release lane stalled — it did, at `0.1.0-linux-alpha.112` on 15 July,
while the rolling candidate had moved to `0.11.0-beta.0.197` with two P0 fixes in it — testers
had no way to get the fixes through `apt` and were back to downloading `.deb` files by hand.

The fix is a second suite, **not** a newer `stable`. Pointing `stable` at candidates would make
the word mean nothing and would push unreleased builds at every subscriber. A machine subscribed
to `stable` is never offered a candidate: they are separate suites, and the container gate
asserts exactly that with `apt-cache policy` and `apt-cache madison` on a stable-only root while
the candidate packages sit in the very same pool it is reading.

`candidate`'s identity comes from `candidate.linux.json`, the pointer asset on the rolling
release — the same pointer the `.deb` self-updater and the AppImage lane read, and the one
`okp-core::candidate_build` uploads *last*, after the artifacts it names. So the suite can never
advertise a half-published build, and it always agrees with the candidate feed. Its window is the
pointer's current build plus its history (`MAX_RETAINED_PREVIOUS`, so 6 in total by default); a
history entry whose asset has already been pruned from the release drops out of the suite rather
than becoming an index entry apt cannot download, while a *current* build the release does not
carry aborts the lane.

**The acceptance gate applies here too.** `okp-core::candidate_channel::select_candidate_update_from_feed`
refuses a pointer whose `acceptance` is not `Accepted`, so the installed `.deb` never offers a
`pending` or `rejected` build. apt must not be the one channel that ships it anyway — `apt upgrade`
would push a build that failed acceptance to every subscribed tester, without asking. A
non-accepted current build is therefore withheld and the suite falls back to the newest accepted
build in the pointer's history (okp-core only ever admits accepted builds there). With nothing
accepted to fall back to, the archive is published `stable`-only.

**Candidate packages are checked against the pointer's digest.** The rolling release is explicitly
mutable — assets are replaced in place — so an asset could be swapped after the pointer that names
it was written, and nothing downstream would notice: the archive is signed over whatever ended up
in the pool, and the container gate derives its expectations from that same index. Each candidate
`.deb` is therefore hashed after download and compared with the `sha256` the pointer publishes for
it, before anything is signed. `stable` has no such column and needs none: a `linux-v*` release
asset is immutable, and the releases API publishes no digest to check it against anyway.

Setting `OKP_APT_CANDIDATE_MAX_VERSIONS=0`, or having no `linux-candidate` release at all,
publishes a `stable`-only archive — including no `ok-player-candidate.sources`, because apt fails
hard on a source line naming a suite the archive does not have. "No candidate release" is decided
by a 404 and nothing else: any other API failure aborts the lane rather than quietly un-publishing
the channel testers are subscribed to.

### Why `candidate` is a plain suite, not `NotAutomatic`

Debian's backports idiom (`NotAutomatic: yes` + `ButAutomaticUpgrades: yes`) would let a machine
carry both source stanzas and still stay on releases until it asked for a candidate. It is
deliberately not used here, for now. It works by manipulating pin priorities, and the behaviour
this lane must guarantee — `apt upgrade` moving a subscribed machine from one rolling build to the
next — is exactly what those priorities make subtle. The plain suite is what the container gate
actually proves, on the configuration the docs actually recommend (candidate *instead of* stable,
not beside it). Revisit it if testers turn out to want both channels on one machine; it would need
its own container case for the upgrade path before it could be trusted.

### One pool, one key, one generator

Both suites are produced by one run of `scripts/build-apt-repo.sh` and signed by one key in one
loop. There is no second archive and no second signing path;
`scripts/tests/feeds-workflow.Tests.sh` asserts that `build-apt-repo.sh` appears exactly once in
the workflow, in the signing job.

They share `pool/`. The pool is indexed **once** with `dpkg-scanpackages`, and each suite's
`Packages` is a subset of that single index, so a package both suites carry is stored once and
has literally the same paragraph — same `Size`, same `SHA256` — in both. The rolling-window
budget charges each distinct pool file once for the same reason; counting a shared package twice
would shrink both windows for bytes that are not there. A pool file no suite indexes aborts the
lane: it is dead weight against the budget and invisible to apt.

## The package provisions the repository (issue #726)

Publishing an archive is only half of it. A `.deb` downloaded from the releases page used to
install a player that could never update itself: its `postinst` refreshed the desktop and icon
caches and nothing else, so the machine ended with OK Player installed and **no OK Player apt
source**. That is not a hypothetical — it is what the operator measured in #725, where
`apt-cache policy ok-player` listed exactly one origin, `/var/lib/dpkg/status`, while the app
advised updating through apt and the desktop updater it offered correctly answered that
everything was up to date. Three days were spent on a stale build pressing buttons that could
not work.

So the package provisions the repository itself, the way Chrome, VS Code and Docker's `.deb`
files do, because asking a user to add a source after installing is a step most will never take
and the ones who do will still land on the wrong suite.

`scripts/package-linux-deb.sh` carries two files under `/usr/share/ok-player/apt/`: the archive
key, dearmored from the committed `rust/packaging/linux/ok-player-archive-keyring.asc`, and the
deb822 stanza for this artifact's suite. `postinst` copies them to
`/usr/share/keyrings/ok-player-archive-keyring.gpg` and
`/etc/apt/sources.list.d/ok-player.sources`. Nothing is fetched at install time — a package that
had to reach the network for a key would fail on exactly the machines that cannot.

Four rules carry it:

* **The suite matches the artifact.** `OKP_DEB_APT_SUITE` decides, `release-linux.yml` sets it
  to `stable`, and everything else defaults to `candidate`. The default is the honest
  description of an undeclared build, and defaulting the other way would silently move a tester
  who installed a candidate `.deb` onto `stable` — the same class of failure as #725, from the
  other side. `scripts/tests/deb-apt-provisioning.Tests.sh` fails if the release lane ever stops
  declaring itself.
* **The key is asserted at build time.** The committed key's fingerprint must equal
  `77D0FCDEB0D594E13E50F43A9337815EB0F78C63` or the packaging aborts, so a package can never
  ship a key that cannot verify the archive it points at. That would be worse than shipping no
  key: apt would fail `update` outright rather than merely not update.
* **An existing choice is never overwritten.** If any configured source can deliver packages
  from the archive — this file, the candidate stanza, or a hand-written one-line entry —
  `postinst` leaves it alone. "Can deliver" is the whole test, and it is narrower than "mentions
  the URL": an entry that is commented out, one turned off with `Enabled: no`, and a source-only
  entry (`deb-src`, or a stanza whose `Types` omits `deb`) all build no Packages index, so none
  of them counts. Treating one as a subscription would leave the machine with an entry that
  delivers nothing and no working source beside it — the state this whole change exists to
  prevent. The keyring, by contrast, is refreshed unconditionally: it is this package's copy of
  the key the archive is signed with, and a machine that missed a rotation could not verify the
  archive at all.
* **`purge` removes both, `remove` does not.** That is how apt-repository-shipping packages
  behave, and it is what lets a reinstall skip re-adding the source. The keyring is the
  exception to the exception: `purge` removes the package's own stanza first and then keeps the
  keyring if any OK Player source is still configured. A user who subscribed through a file of
  their own keeps a source whose `Signed-By` names it, and taking it out from under them would
  fail `apt update` for the whole machine rather than only for OK Player.

**Neither file is a dpkg conffile, deliberately.** A conffile would give "local edits survive an
upgrade", but it cannot give what matters more here. dpkg compares a conffile against the md5 of
what the *previous package* shipped, so a stanza the user never edited is replaced silently —
which is exactly a `candidate` subscriber being put back on `stable` by their next stable
install. The files are therefore owned by the maintainer scripts (Debian Policy 10.7.3), which
preserves both a local edit and an untouched deliberate choice.

### Verifying it

`scripts/verify-deb-apt-provisioning.sh` is the acceptance, and it asserts apt's own answer on a
real machine: install the `.deb` from a file in a clean `debian:13-slim` container, run
`apt-get update`, and require `apt-cache policy ok-player` to name the archive as a source and
offer a newer build, with no step in between. It runs four scenarios — `stable`, `candidate`, an
existing subscription surviving a stable install, and the negative control — against a real
signed archive built by the generator above and served over `file://` from a bind mount. The
negative control is not a package with a broken `postinst`: it is the pre-#726 package, its
carried files removed and its `postinst` put back to the cache refresh it used to be, and it
must leave `/var/lib/dpkg/status` as the only origin — the exact state the operator was in.

`scripts/tests/deb-apt-provisioning.Tests.sh` is the fast half, with no container and no
network: it runs the packaging for real against a fixture root and asks the produced packages
and their maintainer scripts what they carry and what they do. It also compares the stanza the
package installs against the one this generator publishes for the same suite, byte for byte, by
sourcing the generator and asking it to write one — which is what keeps the two from drifting.

## The version scheme (issue #709)

A `.deb` does not carry the build version. It carries the **Debian encoding** of it:

```text
build version   0.11.0-beta.0.209        what About reports, what the feeds carry,
                                         what the artifact file is named
Version:        1:0.11.0~beta.0.209      what dpkg and apt compare
```

The rule is one line, and `scripts/linux-package-version.sh` owns it for the packaging while
`okp_core::package_version` owns it for the shells that read a version back:

> **`1:` + the build version with every `-` replaced by `~`.**

Both halves are load-bearing, and each was measured with `dpkg --compare-versions` (dpkg 1.23.7)
rather than reasoned about:

| claim | verdict |
|---|---|
| `0.11.0-beta.0.208 gt 0.11.0` | **true** — dpkg reads the tail after the last `-` as a *Debian revision*, and a revision outranks its absence. Every `.deb` published under the build version sits above the release that follows it, so the APT lane could never ship a stable version to anybody. |
| `0.11.0-beta.0.208 lt 0.11.0~beta.0.209` | **false** — `~` fixes the ordering against the release, but the corrected string now sorts *below* what testers already have installed, and apt refuses it as a downgrade. Every existing candidate subscriber would be stranded. |
| `0.11.0-beta.0.208 lt 1:0.11.0~beta.0.209` | **true** — an epoch outranks anything without one, whatever it looks like. |
| `1:0.11.0~beta.0.209 lt 1:0.11.0` | **true** — the release outranks the candidates that led to it. |
| `1:0.11.0~beta.0.9 lt 1:0.11.0~beta.0.10` | **true** — candidates still order among themselves across a decimal boundary. |
| `1:0.11.0-beta.0.208 lt 1:0.11.0` | **false** — the epoch alone does not fix anything; the `~` is what orders a prerelease below its release. |

The epoch is **permanent**. Removing it later would strand exactly the people it was added for.
It should also be the only one this project ever needs: `~` fixes the ordering itself, so no
future release has to climb over its own prereleases with a second epoch.

Every `-` is replaced, not just the first, so an encoded version never carries a Debian revision
at all — `0.1.0-linux-alpha.112` becomes `1:0.1.0~linux~alpha.112`. A build version may not
contain `~` or `:` (the packaging refuses one that does), which makes the substitution a
bijection: `okp_build_version_from_debian` recovers the exact build a package was made from.
That is what lets the installed-build watch (#707/#708) compare `dpkg-query`'s answer against
the version the running session reports without a second comparator, and it is why a version
this packaging never emits — no epoch, or somebody's rebuild with a real revision — is refused
rather than guessed at.

**File names are deliberately not encoded.** Pool files stay
`ok-player_0.11.0-beta.0.209_amd64.deb`, named by the build, exactly like the AppImage, the
release tag and the `candidate.linux.json` pointer beside them. A Debian pool file name never
had to match the version inside it, and one identity for the artifact is worth more than
matching a convention.

The rpm lane shares the substitution and takes **no** epoch: rpm forbids `-` in a version
outright, so that lane always emitted `0.11.0~beta.1` and its ordering was never wrong. Adding
an epoch there would be a permanent cost for nothing. `scripts/package-linux-rpm-source.sh` now
derives `rpm_version` from `upstream_version` through the same function, so the spec can no
longer keep a stale hand-maintained default.

`scripts/verify-apt-repo.sh` asserts this over every paragraph of every suite, before any
container starts: a version must be exactly the encoding of the build its pool file is named
for, must outrank `0.11.0-beta.0.208` (the newest build published before the encoding existed),
and, if it is a prerelease, must sort below its own release.

Packages published before the encoding are counted and reported rather than failed — the archive
is rebuilt from the release assets, so they stay in the rolling window until they age out, and
the epoch is what keeps them below everything encoded. That exception is narrow on purpose: the
`Version:` must be *literally* the build version its pool file is named for, the build must be a
prerelease, and it must be at or below `0.11.0-beta.0.208`. The prerelease condition is the one
that is easy to miss — dpkg ranks a raw `0.11.0` **below** `0.11.0-beta.0.208`, so an exception
phrased only as "at or below the high-water mark" would wave through exactly the unencoded
release that no candidate subscriber could install.

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
  libmpv is ~76 MB, so the archive is a rolling window. Each suite keeps its own newest builds —
  `OKP_APT_MAX_VERSIONS` (default 10) for `stable`, `OKP_APT_CANDIDATE_MAX_VERSIONS` (default 6)
  for `candidate` — and both draw on one shared `OKP_APT_POOL_BUDGET_BYTES` (default 900 MiB),
  charged per distinct pool file. Versions outside the window drop out of the pool and the index
  *together*, so the archive never carries a dangling reference, and they remain downloadable
  from their own GitHub release — which is what the standalone `.deb` self-updater already uses.
  The window is the one place where "additive" is bounded by physics; everything inside it is
  strictly additive.

  The **current build of each suite is not optional**. If it cannot fit — because the budget is
  too small or the version count is — the lane aborts rather than trimming it and keeping its
  predecessors. A signed archive advertising an older version than the JSON feeds do is worse
  than no publish at all, because apt clients would accept it.

  After both current builds are reserved, the remaining budget is offered to the two tails **in
  turn**, not to one suite and then the other. A long release history must not be able to starve
  the QA channel down to a single build, or the other way round.

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
so nothing has to be deployed first. It runs **once per suite the archive carries**, subscribing
to exactly one suite at a time the way a real machine is configured: it adds the published
keyring and that suite's source, runs `apt-get update` and `apt-get install ok-player`, requires
apt to select the version that suite advertises, then starts `/usr/bin/ok-player` headless and
requires it to reach its GUI initialisation (the process then dies on the absent display, which
is expected — getting that far is the "the packaged binary and its dependency closure are real"
signal used by the other Linux gates).

Between the two it asserts the channel separation directly: with only `stable` subscribed, and
the candidate packages sitting in the same pool apt is reading from, `apt-cache policy` and
`apt-cache madison` must not mention any version that `candidate` carries and `stable` does not,
and `ok-player` must resolve to the release version. That is the assertion that separation is a
property of the archive rather than of which packages happen to be published this week.

The check is deliberately about *candidate-only* versions rather than about the candidate head's
version string. Overlap between the suites is legitimate — a candidate promoted unchanged is
literally the same pool file in both — and in that case the stable index rightly contains the
candidate's version. Asserting on the version string would fail the whole lane the first time a
promotion happened, over a package that came from `stable`. With full overlap the candidate-only
set is empty and there is nothing that could leak, which the gate says out loud rather than
passing silently.

Which signal is required is derived from the packaged binary rather than fixed. Current builds
log `Renderer policy:` as the first statement of `main()`, before GTK, so for them that line is
mandatory. Releases published before that line existed — the newest published `linux-v*` release
is one — can only prove they reached GTK's display connection, and the archive has to stay
verifiable over the versions it actually carries. An unresolved shared library is a hard failure
either way, which is what this check exists to catch.

It then runs two negative controls, **per suite**. The wrong-key control repeated per suite is
also what proves both suites are signed by the same key: a suite signed by some other key would
still be accepted against a keyring holding that other key. Both controls check *two* things,
because by default `apt-get update`
reports a rejected repository as a warning, keeps the previously fetched index and still exits 0
— asserting on its exit status alone would pass even if apt had happily used a forged index. So
each control asserts that the update fails under `APT::Update::Error-Mode=any` **and** that, with
every cached index discarded first, the package is genuinely not installable:

* a tampered `InRelease` is refused;
* an archive whose signature does not match the keyring is refused.

Given a second, older archive it additionally proves the upgrade path, per suite: install from
the old archive, publish the new one over the same repository, and require `apt-get upgrade` to
move to the newer version while the older one stays installable by explicit version. For
`candidate` that is the acceptance criterion of #689 — a tester's machine moving from one rolling
build to the next through `apt upgrade` alone.

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
* **`release-linux-candidate.yml` calls it too**, after a rolling publication. This matters more
  than it looks: replacing assets on the mutable `linux-candidate` release fires no event *Update
  Feeds* listens for, so without that call a new candidate would sit in the release, be offered by
  the `.deb` self-updater, and be invisible to `apt upgrade` until some unrelated deploy happened
  to run — which is the exact "testers cannot get it through apt" failure this suite exists to
  end. The call is gated on `publish_result == 'published'`: the candidate workflow runs every 15
  minutes and most runs publish nothing, which must not cost a Pages deploy. A refresh failure
  does not unpublish the candidate; the rolling release is already live and the refresh is
  idempotent and re-runnable. The candidate lane's `linux-candidate-native` concurrency group was
  moved from the workflow to the building job when that call was added: a workflow-level group
  covers every job in the run, so a Pages deploy that could not start — signing runner offline,
  secret store down — would have stopped candidate builds entirely. The QA lane must not be
  blocked by the publication of its own archive.
* A tester subscribes to the QA channel by installing `ok-player-candidate.sources` **instead
  of** `ok-player.sources`, not beside it: with both files present apt sees both suites and will
  offer the candidate anyway, which makes "I am on stable" untrue without saying so. Both stanzas
  point at the same keyring, so switching channels never means re-trusting a key.
* Rotating the signing key means updating the four Infisical secrets **and** the two committed
  values the packaging asserts against — `rust/packaging/linux/ok-player-archive-keyring.asc`
  and `OKP_APT_SIGNING_FINGERPRINT` in `scripts/apt-archive-identity.sh` — in the same change,
  then letting the next run republish `ok-player-archive-keyring.{asc,gpg}`. The packaging
  refuses to build a `.deb` whose key does not match that fingerprint, so a half-done rotation
  stops the Linux lane rather than shipping a package that cannot verify the archive. Existing installs need the new key before the
  first archive signed with it is served, so publish a run with the *old* key still in place
  after users have fetched the new keyring, or accept that clients must re-fetch it.
* `scripts/tests/apt-repo-generator.Tests.sh` runs in the *Rust* workflow next to the other
  script-level policy suites. It needs no network, no Infisical and no container: it builds real
  `.deb` files with `dpkg-deb` and drives the real generator with a throwaway key.
