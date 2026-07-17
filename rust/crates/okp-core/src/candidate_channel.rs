//! Candidate update channel for the Linux QA lane (issue #339).
//!
//! The public Linux `.deb` lane discovers updates from `deb.linux.json`, one
//! manifest derived from the newest *published* `linux-v*` GitHub Release
//! ([`crate::update_selection`]). That model creates one permanent Release per
//! build, which is wrong for QA candidates: they are development checkpoints,
//! not products, and the repository already carries more than a hundred of
//! them. This module owns a *separate* channel for explicitly enrolled QA
//! installs that can update frequently without minting a Release per build.
//!
//! Isolation is the whole point. The candidate channel:
//! - has its own feed schema ([`CandidateFeed`]) and its own feed URL, served
//!   from a single mutable "rolling" publication surface rather than a new
//!   Release per build (see `scripts/build-linux-candidate-feed.sh`);
//! - is consulted only by an install that has *explicitly* enrolled
//!   (`Settings.updates.channel == UpdateChannel::Candidate`, or the
//!   `OKP_LINUX_UPDATE_CHANNEL=candidate` override); a default install never
//!   fetches it, so the public feed and its user behavior are untouched;
//! - carries, per build, the exact git SHA, monotonic build number, UTC
//!   timestamp, artifact SHA-256, and acceptance status, so an enrolled install
//!   can prove what it is about to install and refuse anything not yet accepted.
//!
//! Monotonic ordering reuses [`crate::update_selection::compare_versions`]; the
//! SemVer identities the channel walks — `0.11.0-beta.0.<build>` before public
//! beta 1, `0.11.0-beta.1` at beta 1, `0.11.0-beta.1.<build>` after, then
//! `0.11.0-beta.2` — already sort in that order under the shared comparison, and
//! the tests below pin the whole transition.

use serde::{Deserialize, Serialize};

use crate::sha256sums::Sha256Sums;
use crate::update_selection::compare_versions;
use std::cmp::Ordering;

/// The channel name every candidate feed must declare. A feed whose `channel`
/// is anything else is not a candidate feed and is refused, so a public feed can
/// never be mistaken for — or served as — a candidate feed.
pub const CANDIDATE_CHANNEL: &str = "candidate";

/// Minimum number of *previous* known-good full packages the rolling surface
/// keeps alongside the current candidate, so an enrolled install always has at
/// least two builds to roll back to when a candidate misbehaves. The publisher
/// (`scripts/build-linux-candidate-feed.sh`) prunes to `current + this` and the
/// [`CandidateFeed::has_sufficient_recovery`] check pins the invariant.
pub const MIN_RETAINED_PREVIOUS: usize = 2;

/// Acceptance state of a candidate build. Only [`AcceptanceStatus::Accepted`]
/// builds are offered to enrolled installs; `Pending` (evidence not yet
/// complete) and `Rejected` (failed acceptance) are discoverable in the feed for
/// operators but never selected for install, so a half-verified candidate can
/// sit on the rolling surface without being pushed to the fleet.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcceptanceStatus {
    Pending,
    Accepted,
    Rejected,
}

/// A single candidate package on the rolling surface: enough to download, verify,
/// and identify a build. `sha256` is the artifact's own digest, carried in the
/// feed so an enrolled install can reject a feed/package mismatch (see
/// [`CandidatePackage::matches_sums`]) before it ever hands the file to a
/// privileged installer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidatePackage {
    /// File name of the `.deb`, e.g. `ok-player_0.11.0-beta.0.108_amd64.deb`.
    pub name: String,
    /// Absolute download URL of the package on the rolling publication surface.
    pub url: String,
    /// Expected size in bytes, a download sanity check when present.
    #[serde(default)]
    pub size: Option<u64>,
    /// Lowercase hex SHA-256 of the package bytes, as published in the feed.
    pub sha256: String,
}

/// Velopack full-package identity for an AppImage candidate. These fields map
/// directly to one `releases.linux-candidate.json` full asset, but live in the
/// candidate manifest so both Linux package lanes are gated by the same
/// accepted, atomically-published pointer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateAppImage {
    pub package_id: String,
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub sha1: String,
}

/// One previous accepted candidate retained as a complete recovery point for
/// both Linux package lanes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateHistoryEntry {
    pub version: String,
    pub build: u64,
    pub package: CandidatePackage,
    pub appimage: CandidateAppImage,
    pub sha256sums_url: String,
}

impl CandidatePackage {
    /// Rejects a feed/package SHA mismatch: the digest this feed *declares* for
    /// the package must equal the digest `SHA256SUMS` *publishes* for the same
    /// file name. A mismatch means the feed and the checksum manifest disagree
    /// about what the package is — a partial or tampered promotion — and the
    /// install must fail closed rather than trust either side.
    pub fn matches_sums(&self, sums: &Sha256Sums) -> Result<(), CandidateIdentityError> {
        let expected =
            sums.expected_hex(&self.name)
                .ok_or_else(|| CandidateIdentityError::FileNotListed {
                    file_name: self.name.clone(),
                })?;
        if expected.eq_ignore_ascii_case(&self.sha256) {
            Ok(())
        } else {
            Err(CandidateIdentityError::Sha256Mismatch {
                file_name: self.name.clone(),
                feed: self.sha256.clone(),
                sums: expected.to_owned(),
            })
        }
    }
}

/// Why a candidate package failed identity verification against `SHA256SUMS`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateIdentityError {
    /// `SHA256SUMS` has no entry for the feed's package.
    FileNotListed { file_name: String },
    /// The feed's declared digest differs from the one in `SHA256SUMS`.
    Sha256Mismatch {
        file_name: String,
        feed: String,
        sums: String,
    },
}

impl std::fmt::Display for CandidateIdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotListed { file_name } => {
                write!(f, "SHA256SUMS has no entry for candidate {file_name}")
            }
            Self::Sha256Mismatch {
                file_name,
                feed,
                sums,
            } => write!(
                f,
                "candidate {file_name} SHA mismatch: feed declares {feed}, SHA256SUMS lists {sums}"
            ),
        }
    }
}

impl std::error::Error for CandidateIdentityError {}

/// The rolling candidate feed (`candidate.linux.json`). Unlike the public
/// `deb.linux.json`, exactly one of these is served at a time from a mutable
/// surface, and it advertises not just the newest package but the retained
/// previous known-good packages ([`CandidateFeed::history`]) so an enrolled
/// install can roll back without hunting the release list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateFeed {
    /// Must equal [`CANDIDATE_CHANNEL`]; a mismatch means this is not a
    /// candidate feed and is refused.
    pub channel: String,
    /// Monotonic SemVer identity, e.g. `0.11.0-beta.0.108`.
    pub version: String,
    /// Monotonic build number; strictly increases across candidate builds.
    pub build: u64,
    /// Exact git commit SHA the candidate was built from.
    pub commit_sha: String,
    /// RFC 3339 UTC build timestamp, e.g. `2026-07-17T09:41:00Z`.
    pub timestamp_utc: String,
    /// Acceptance state; only [`AcceptanceStatus::Accepted`] is offered.
    pub acceptance: AcceptanceStatus,
    /// The current candidate package.
    pub package: CandidatePackage,
    /// The exact Velopack full package used by an enrolled AppImage install.
    pub appimage: CandidateAppImage,
    /// Absolute URL of the candidate `SHA256SUMS`; the shell verifies the
    /// download against it before installing.
    #[serde(default)]
    pub sha256sums_url: Option<String>,
    /// Previous known-good packages retained for rollback, newest first. The
    /// publisher keeps at least [`MIN_RETAINED_PREVIOUS`] of these.
    #[serde(default)]
    pub history: Vec<CandidateHistoryEntry>,
}

impl CandidateFeed {
    /// True when this feed declares the candidate channel. A feed that fails
    /// this is not consulted — the shell must never treat a non-candidate
    /// document as a candidate feed.
    pub fn is_candidate_channel(&self) -> bool {
        self.channel == CANDIDATE_CHANNEL
    }

    /// True when the rolling surface still carries enough previous known-good
    /// packages to recover from a bad candidate.
    pub fn has_sufficient_recovery(&self) -> bool {
        self.history.len() >= MIN_RETAINED_PREVIOUS
    }

    /// The accepted pointer must carry complete cryptographic identities for
    /// both package lanes. In particular, an empty AppImage SHA256 must not
    /// make Velopack fall back to the weaker legacy SHA1 field.
    pub fn has_valid_identity(&self) -> bool {
        is_hex(&self.commit_sha, 40)
            && is_hex(&self.package.sha256, 64)
            && is_hex(&self.appimage.sha256, 64)
            && !self.package.name.trim().is_empty()
            && !self.package.url.trim().is_empty()
            && !self.appimage.name.trim().is_empty()
            && !self.appimage.url.trim().is_empty()
            && !self.appimage.package_id.trim().is_empty()
    }
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A candidate selected from the feed, ready for the shell to download, verify,
/// and install — the candidate analogue of
/// [`crate::update_selection::DebUpdate`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateUpdate {
    pub version: String,
    pub build: u64,
    pub commit_sha: String,
    pub package: CandidatePackage,
    pub appimage: CandidateAppImage,
    pub sums_url: Option<String>,
}

/// Selects the feed's candidate when an *enrolled* install should install it:
/// the feed is a candidate feed, the build is `Accepted`, and its version is
/// strictly newer than `current_version`. Returns `None` otherwise — a
/// not-newer, pending, rejected, or non-candidate feed all mean "nothing to
/// install", kept distinct from a failed fetch (the shell's concern), exactly as
/// the public lane keeps "up to date" distinct from "couldn't check".
///
/// Because selection is one monotonic version comparison, two sequential
/// candidate builds are applied in order: an install on `beta.0.108` takes
/// `beta.0.109`, and once on `109` it takes `beta.1` — it never skips or
/// reorders, and it never steps backward onto a rolled-back candidate.
pub fn select_candidate_update_from_feed(
    feed: CandidateFeed,
    current_version: &str,
) -> Option<CandidateUpdate> {
    if !feed.is_candidate_channel() {
        return None;
    }
    if feed.acceptance != AcceptanceStatus::Accepted {
        return None;
    }
    if !feed.has_valid_identity() {
        return None;
    }
    if compare_versions(&feed.version, current_version) != Ordering::Greater {
        return None;
    }
    Some(CandidateUpdate {
        version: feed.version,
        build: feed.build,
        commit_sha: feed.commit_sha,
        package: feed.package,
        appimage: feed.appimage,
        sums_url: feed.sha256sums_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str, sha256: &str) -> CandidatePackage {
        CandidatePackage {
            name: format!("ok-player_{version}_amd64.deb"),
            url: format!("https://example.invalid/rolling/ok-player_{version}_amd64.deb"),
            size: Some(4242),
            sha256: sha256.to_owned(),
        }
    }

    fn appimage(version: &str, sha256: &str) -> CandidateAppImage {
        CandidateAppImage {
            package_id: "com.befeast.okplayer".to_owned(),
            name: format!("com.befeast.okplayer-{version}-linux-candidate-full.nupkg"),
            url: format!(
                "https://example.invalid/rolling/com.befeast.okplayer-{version}-linux-candidate-full.nupkg"
            ),
            size: 8484,
            sha256: sha256.to_owned(),
            sha1: "1".repeat(40),
        }
    }

    fn history(version: &str, build: u64, digest: char) -> CandidateHistoryEntry {
        CandidateHistoryEntry {
            version: version.to_owned(),
            build,
            package: package(version, &digest.to_string().repeat(64)),
            appimage: appimage(version, &digest.to_ascii_uppercase().to_string().repeat(64)),
            sha256sums_url: format!("https://example.invalid/rolling/SHA256SUMS-{build}.txt"),
        }
    }

    fn feed(version: &str, build: u64, acceptance: AcceptanceStatus) -> CandidateFeed {
        CandidateFeed {
            channel: CANDIDATE_CHANNEL.to_owned(),
            version: version.to_owned(),
            build,
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            timestamp_utc: "2026-07-17T09:41:00Z".to_owned(),
            acceptance,
            package: package(version, &"a".repeat(64)),
            appimage: appimage(version, &"f".repeat(64)),
            sha256sums_url: Some(format!(
                "https://example.invalid/rolling/SHA256SUMS-{build}.txt"
            )),
            history: vec![
                history("0.11.0-beta.0.107", 107, 'c'),
                history("0.11.0-beta.0.106", 106, 'b'),
            ],
        }
    }

    #[test]
    fn semver_transition_from_candidate_to_beta_one_is_monotonic() {
        // The exact identity ladder from the issue, in publication order.
        let ladder = [
            "0.11.0-beta.0.108",
            "0.11.0-beta.0.109",
            "0.11.0-beta.1",
            "0.11.0-beta.1.3",
            "0.11.0-beta.2",
        ];
        for pair in ladder.windows(2) {
            assert_eq!(
                compare_versions(pair[1], pair[0]),
                Ordering::Greater,
                "{} must sort after {}",
                pair[1],
                pair[0]
            );
        }
        // The load-bearing step: the last pre-beta.1 candidate is older than the
        // public beta.1 identity, so an enrolled install rolls forward onto it.
        assert_eq!(
            compare_versions("0.11.0-beta.1", "0.11.0-beta.0.109"),
            Ordering::Greater
        );
    }

    #[test]
    fn two_sequential_candidate_builds_are_applied_in_order() {
        // On beta.0.108, the next accepted build beta.0.109 is offered.
        let step_one = select_candidate_update_from_feed(
            feed("0.11.0-beta.0.109", 109, AcceptanceStatus::Accepted),
            "0.11.0-beta.0.108",
        )
        .expect("a newer accepted candidate should be selected");
        assert_eq!(step_one.version, "0.11.0-beta.0.109");
        assert_eq!(step_one.build, 109);

        // Now on beta.0.109, the promotion to the public beta.1 identity is
        // offered next — in order, without skipping.
        let step_two = select_candidate_update_from_feed(
            feed("0.11.0-beta.1", 110, AcceptanceStatus::Accepted),
            "0.11.0-beta.0.109",
        )
        .expect("the beta.1 promotion should be selected next");
        assert_eq!(step_two.version, "0.11.0-beta.1");

        // Already on beta.1: the same feed offers nothing, so there is no loop.
        assert!(
            select_candidate_update_from_feed(
                feed("0.11.0-beta.1", 110, AcceptanceStatus::Accepted),
                "0.11.0-beta.1"
            )
            .is_none()
        );
    }

    #[test]
    fn pending_or_rejected_candidate_is_never_offered() {
        assert!(
            select_candidate_update_from_feed(
                feed("0.11.0-beta.0.200", 200, AcceptanceStatus::Pending),
                "0.11.0-beta.0.108"
            )
            .is_none(),
            "a pending candidate must not be installed even when newer"
        );
        assert!(
            select_candidate_update_from_feed(
                feed("0.11.0-beta.0.200", 200, AcceptanceStatus::Rejected),
                "0.11.0-beta.0.108"
            )
            .is_none(),
            "a rejected candidate must not be installed even when newer"
        );
    }

    #[test]
    fn a_non_candidate_feed_is_refused() {
        let mut public = feed("0.11.0-beta.0.200", 200, AcceptanceStatus::Accepted);
        public.channel = "linux".to_owned();
        assert!(!public.is_candidate_channel());
        assert!(
            select_candidate_update_from_feed(public, "0.11.0-beta.0.108").is_none(),
            "a feed that does not declare the candidate channel must never be selected"
        );
    }

    #[test]
    fn incomplete_appimage_identity_is_refused() {
        let mut invalid = feed("0.11.0-beta.1.42", 42, AcceptanceStatus::Accepted);
        invalid.appimage.sha256.clear();
        assert!(!invalid.has_valid_identity());
        assert!(select_candidate_update_from_feed(invalid, "0.11.0-beta.1.41").is_none());
    }

    #[test]
    fn rolling_surface_keeps_at_least_two_previous_packages() {
        assert!(
            feed("0.11.0-beta.0.108", 108, AcceptanceStatus::Accepted).has_sufficient_recovery()
        );

        let mut thin = feed("0.11.0-beta.0.108", 108, AcceptanceStatus::Accepted);
        thin.history.truncate(1);
        assert!(
            !thin.has_sufficient_recovery(),
            "one retained package is not enough to guarantee rollback"
        );
    }

    #[test]
    fn feed_package_rejects_sha_mismatch_against_sums() {
        let digest = "d".repeat(64);
        let pkg = package("0.11.0-beta.0.108", &digest);
        let good = Sha256Sums::parse(&format!("{digest}  {}\n", pkg.name)).expect("valid manifest");
        assert!(pkg.matches_sums(&good).is_ok());

        let other = "e".repeat(64);
        let bad = Sha256Sums::parse(&format!("{other}  {}\n", pkg.name)).expect("valid manifest");
        assert_eq!(
            pkg.matches_sums(&bad),
            Err(CandidateIdentityError::Sha256Mismatch {
                file_name: pkg.name.clone(),
                feed: digest.clone(),
                sums: other,
            })
        );

        let unrelated =
            Sha256Sums::parse(&format!("{digest}  something-else.deb\n")).expect("valid manifest");
        assert!(matches!(
            pkg.matches_sums(&unrelated),
            Err(CandidateIdentityError::FileNotListed { .. })
        ));
    }

    #[test]
    fn candidate_feed_parses_rolling_manifest_json() {
        let json = r#"{
            "channel": "candidate",
            "version": "0.11.0-beta.0.108",
            "build": 108,
            "commit_sha": "0123456789abcdef0123456789abcdef01234567",
            "timestamp_utc": "2026-07-17T09:41:00Z",
            "acceptance": "accepted",
            "package": {
                "name": "ok-player_0.11.0-beta.0.108_amd64.deb",
                "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/ok-player_0.11.0-beta.0.108_amd64.deb",
                "size": 12345678,
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "appimage": {
                "package_id": "com.befeast.okplayer",
                "name": "com.befeast.okplayer-0.11.0-beta.0.108-linux-candidate-full.nupkg",
                "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/com.befeast.okplayer-0.11.0-beta.0.108-linux-candidate-full.nupkg",
                "size": 7654321,
                "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                "sha1": "1111111111111111111111111111111111111111"
            },
            "sha256sums_url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/SHA256SUMS-108.txt",
            "history": [
                {
                    "version": "0.11.0-beta.0.107",
                    "build": 107,
                    "package": {
                        "name": "ok-player_0.11.0-beta.0.107_amd64.deb",
                        "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/ok-player_0.11.0-beta.0.107_amd64.deb",
                        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    },
                    "appimage": {
                        "package_id": "com.befeast.okplayer",
                        "name": "com.befeast.okplayer-0.11.0-beta.0.107-linux-candidate-full.nupkg",
                        "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/com.befeast.okplayer-0.11.0-beta.0.107-linux-candidate-full.nupkg",
                        "size": 7000,
                        "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                        "sha1": "2222222222222222222222222222222222222222"
                    },
                    "sha256sums_url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/SHA256SUMS-107.txt"
                },
                {
                    "version": "0.11.0-beta.0.106",
                    "build": 106,
                    "package": {
                        "name": "ok-player_0.11.0-beta.0.106_amd64.deb",
                        "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/ok-player_0.11.0-beta.0.106_amd64.deb",
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    },
                    "appimage": {
                        "package_id": "com.befeast.okplayer",
                        "name": "com.befeast.okplayer-0.11.0-beta.0.106-linux-candidate-full.nupkg",
                        "url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/com.befeast.okplayer-0.11.0-beta.0.106-linux-candidate-full.nupkg",
                        "size": 6000,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "sha1": "3333333333333333333333333333333333333333"
                    },
                    "sha256sums_url": "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/SHA256SUMS-106.txt"
                }
            ]
        }"#;
        let feed: CandidateFeed =
            serde_json::from_str(json).expect("rolling manifest should parse");
        assert!(feed.is_candidate_channel());
        assert_eq!(feed.build, 108);
        assert_eq!(feed.acceptance, AcceptanceStatus::Accepted);
        assert!(feed.has_sufficient_recovery());
        assert_eq!(feed.package.size, Some(12345678));

        let update = select_candidate_update_from_feed(feed, "0.11.0-beta.0.107")
            .expect("beta.0.108 is newer than beta.0.107");
        assert_eq!(update.package.name, "ok-player_0.11.0-beta.0.108_amd64.deb");
        assert_eq!(
            update.appimage.name,
            "com.befeast.okplayer-0.11.0-beta.0.108-linux-candidate-full.nupkg"
        );
        assert_eq!(
            update.commit_sha,
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert!(update.sums_url.is_some());
    }
}
