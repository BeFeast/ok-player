//! Update-selection logic for the Linux self-update flow, extracted from the
//! Linux GTK shell (EPIC #134, B8) and migrated to a static feed (issue #162,
//! symmetric to the Windows static feed in #131). Both Linux lanes now discover
//! updates from a manifest served on GitHub Pages instead of listing GitHub
//! releases, so a burst of releases on the other track can no longer bury the
//! newest Linux release out of a discovery window (the symmetric failure of
//! #130). This module owns the `.deb` lane's static-manifest schema
//! (`DebFeed`) and the natural version comparison that decides whether the
//! manifest is newer than the running build; the Velopack AppImage lane reads
//! its own `releases.linux.json` through Velopack's `HttpSource`. Windows
//! updates go through Velopack's static feed, so this has no C# counterpart;
//! the spec is the shell tests that moved into this module. Network fetch,
//! checksum verification, and installation stay in the shell. See the
//! compatibility note in docs/core-compatibility.md.

use std::cmp::Ordering;

use serde::Deserialize;

use crate::settings::{SkippedUpdateVersions, UpdateChannel};

/// Name of the checksum-manifest asset every Linux release publishes; the
/// selected update carries its URL so the shell can verify the download.
pub const SHA256SUMS_ASSET: &str = "SHA256SUMS";

/// The static `.deb` update manifest served at `updates/linux/deb.linux.json`
/// on GitHub Pages. `scripts/build-linux-feed.sh` re-derives it from the newest
/// published `linux-v*` release, naming exactly that release's
/// `ok-player_*_amd64.deb` and its `SHA256SUMS`, so the shell never lists GitHub
/// releases to discover an update. Its shape is purpose-built for the `.deb`
/// lane and deliberately differs from the Velopack `releases.linux.json`
/// manifest the AppImage lane reads.
#[derive(Clone, Debug, Deserialize)]
pub struct DebFeed {
    /// Package version of the newest release, e.g. `0.1.0-linux-alpha.108`; the
    /// `linux-v` tag prefix is already stripped by the feed generator.
    pub version: String,
    /// The `.deb` asset to download when the feed is newer than the running
    /// build.
    pub package: DebFeedPackage,
    /// Absolute URL of the release's `SHA256SUMS`; the shell fails closed and
    /// refuses to install when it is absent.
    #[serde(default)]
    pub sha256sums_url: Option<String>,
}

/// The `.deb` package entry of [`DebFeed`].
#[derive(Clone, Debug, Deserialize)]
pub struct DebFeedPackage {
    /// File name of the `.deb`, e.g. `ok-player_0.1.0-linux-alpha.108_amd64.deb`.
    pub name: String,
    /// Absolute GitHub release-asset URL the shell downloads.
    pub url: String,
    /// Expected size in bytes, used as a download sanity check when present.
    #[serde(default)]
    pub size: Option<u64>,
}

/// The `.deb` package selected from the static feed, ready for the shell to
/// download, verify against `sums_url`, and install.
#[derive(Clone, Debug)]
pub struct DebUpdate {
    pub version: String,
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
    pub sums_url: Option<String>,
    /// Candidate feeds bind the package to an exact digest in addition to the
    /// checksum manifest. Public feeds leave this unset and retain their
    /// existing SHA256SUMS-only contract.
    pub expected_sha256: Option<String>,
}

/// Selects the feed's `.deb` when it is strictly newer than `current_version`.
/// The feed already names the single newest published release, so selection is
/// one version comparison — no release listing, no discovery window to fall out
/// of. Returns `None` when the running build is already on the feed's version or
/// newer; a failed feed fetch is the shell's concern and never reaches here, so
/// the empty-feed ("up to date") and failed-fetch ("couldn't check") outcomes
/// stay distinct, as on Windows.
pub fn select_deb_update_from_feed(feed: DebFeed, current_version: &str) -> Option<DebUpdate> {
    if compare_versions(&feed.version, current_version) != Ordering::Greater {
        return None;
    }
    Some(DebUpdate {
        version: feed.version,
        name: feed.package.name,
        url: feed.package.url,
        size: feed.package.size,
        sums_url: feed.sha256sums_url,
        expected_sha256: None,
    })
}

/// Orders package versions with semantic-versioning precedence.
///
/// The numeric core (`0.11.0`, or the four-part `0.11.0.15` a Windows
/// assembly reports) compares run by run, so `…alpha.10` sorts after
/// `…alpha.9`. A version with no prerelease tail outranks every prerelease of
/// the same core — `1.0.0` is newer than `1.0.0-beta.2`, not older because of
/// the extra number. Two prerelease tails compare identifier by identifier,
/// stage before counter: `beta.1` after `beta.0.109`, and `beta.2` before the
/// `beta.2.41` candidates cut from it. Build metadata after `+` carries no
/// precedence, so a checkout stamped `0.1.0-linux-alpha.112+7.gabc1234` is
/// neither newer nor older than the `0.1.0-linux-alpha.112` release. Versions
/// that tie on every count fall back to a lexicographic tiebreak so the order
/// stays total.
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = strip_build_metadata(left);
    let right = strip_build_metadata(right);
    let (left_core, left_pre) = split_prerelease(left);
    let (right_core, right_pre) = split_prerelease(right);
    compare_numeric_runs(left_core, right_core)
        .then_with(|| match (left_pre, right_pre) {
            (None, None) => Ordering::Equal,
            // A release outranks any prerelease of the same core.
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left_pre), Some(right_pre)) => compare_prerelease(left_pre, right_pre),
        })
        .then_with(|| left.cmp(right))
}

/// Splits `1.0.0-beta.1` into its numeric core and its prerelease tail. Build
/// metadata must already be gone: it may itself contain a hyphen.
pub(crate) fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    }
}

/// Drops the `+7.gabc1234` build metadata a checkout build carries; semantic
/// versioning gives it no precedence.
pub(crate) fn strip_build_metadata(version: &str) -> &str {
    version
        .split_once('+')
        .map_or(version, |(before, _)| before)
}

/// Orders two version cores by their numeric runs, a missing run counting as
/// zero.
fn compare_numeric_runs(left: &str, right: &str) -> Ordering {
    let left_key = version_sort_key(left);
    let right_key = version_sort_key(right);
    let max_len = left_key.len().max(right_key.len());
    for index in 0..max_len {
        let left_part = left_key.get(index).copied().unwrap_or_default();
        let right_part = right_key.get(index).copied().unwrap_or_default();
        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            order => return order,
        }
    }
    Ordering::Equal
}

/// Orders two prerelease tails identifier by identifier, so the *stage* is
/// compared before its counter: `alpha.109` precedes `beta.1`, which whole-tail
/// numeric-run comparison gets backwards by looking at 109 against 1 first.
///
/// Within an identifier, two numbers compare numerically (`alpha.9` before
/// `alpha.10`) and anything else compares lexically; a number sorts before a
/// word, and a tail that is a prefix of another sorts before it (`beta.2`
/// before the `beta.2.41` candidate builds cut from it).
fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_part), Some(right_part)) => {
                let order = match (left_part.parse::<u64>(), right_part.parse::<u64>()) {
                    (Ok(left_number), Ok(right_number)) => left_number.cmp(&right_number),
                    // A numeric identifier ranks below an alphanumeric one.
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => left_part.cmp(right_part),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// User-visible lifecycle for one discovered update. The shell retains the
/// verified package/update-manager payload beside this portable projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateOfferPhase {
    Available,
    Skipped,
    Installing,
    InstallFailed(String),
    Installed,
}

/// Portable update decision state shared by automatic and manual checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOfferState {
    channel: UpdateChannel,
    version: String,
    phase: UpdateOfferPhase,
}

impl UpdateOfferState {
    pub fn discovered(
        channel: UpdateChannel,
        version: impl Into<String>,
        skipped_versions: &SkippedUpdateVersions,
    ) -> Self {
        let version = version.into();
        let phase = if skipped_versions.is_skipped(channel, &version) {
            UpdateOfferPhase::Skipped
        } else {
            UpdateOfferPhase::Available
        };
        Self {
            channel,
            version,
            phase,
        }
    }

    pub const fn channel(&self) -> UpdateChannel {
        self.channel
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn phase(&self) -> &UpdateOfferPhase {
        &self.phase
    }

    /// Automatic prompts stay hidden for an exact skipped version. Once the
    /// user has chosen Update, the same surface remains visible for progress
    /// and retryable failure instead of collapsing back into a toast.
    pub fn persistent_surface_visible(&self) -> bool {
        matches!(
            self.phase,
            UpdateOfferPhase::Available
                | UpdateOfferPhase::Installing
                | UpdateOfferPhase::InstallFailed(_)
        )
    }

    pub fn primary_action_label(&self) -> Option<&'static str> {
        match self.phase {
            UpdateOfferPhase::Available | UpdateOfferPhase::InstallFailed(_) => Some("Update"),
            UpdateOfferPhase::Skipped => Some("Install anyway"),
            UpdateOfferPhase::Installing => Some("Updating…"),
            UpdateOfferPhase::Installed => None,
        }
    }

    pub fn can_skip(&self) -> bool {
        matches!(
            self.phase,
            UpdateOfferPhase::Available | UpdateOfferPhase::InstallFailed(_)
        )
    }

    pub fn is_installing(&self) -> bool {
        matches!(self.phase, UpdateOfferPhase::Installing)
    }

    pub fn start_install(&mut self) -> bool {
        if matches!(
            self.phase,
            UpdateOfferPhase::Available
                | UpdateOfferPhase::Skipped
                | UpdateOfferPhase::InstallFailed(_)
        ) {
            self.phase = UpdateOfferPhase::Installing;
            true
        } else {
            false
        }
    }

    pub fn skip(&mut self) -> bool {
        if self.can_skip() {
            self.phase = UpdateOfferPhase::Skipped;
            true
        } else {
            false
        }
    }

    pub fn install_failed(&mut self, error: impl Into<String>) -> bool {
        if matches!(self.phase, UpdateOfferPhase::Installing) {
            self.phase = UpdateOfferPhase::InstallFailed(error.into());
            true
        } else {
            false
        }
    }

    /// The shell handed the package to an external installer but cannot attest
    /// that installation completed. Keep the same offer available so a cancel
    /// or external failure never discards the user's retry path.
    pub fn install_deferred(&mut self) -> bool {
        if matches!(self.phase, UpdateOfferPhase::Installing) {
            self.phase = UpdateOfferPhase::Available;
            true
        } else {
            false
        }
    }

    pub fn install_succeeded(&mut self) -> bool {
        if matches!(self.phase, UpdateOfferPhase::Installing) {
            self.phase = UpdateOfferPhase::Installed;
            true
        } else {
            false
        }
    }
}

fn version_sort_key(version: &str) -> Vec<u64> {
    let mut key = Vec::new();
    let mut current = String::new();
    for character in version.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            key.push(current.parse().unwrap_or_default());
            current.clear();
        }
    }
    if !current.is_empty() {
        key.push(current.parse().unwrap_or_default());
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deb_feed(version: &str, sha256sums_url: Option<&str>) -> DebFeed {
        DebFeed {
            version: version.to_owned(),
            package: DebFeedPackage {
                name: format!("ok-player_{version}_amd64.deb"),
                url: format!("https://example.invalid/ok-player_{version}_amd64.deb"),
                size: Some(42),
            },
            sha256sums_url: sha256sums_url.map(str::to_owned),
        }
    }

    #[test]
    fn version_compare_orders_alpha_numbers_naturally() {
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.10", "0.1.0-linux-alpha.9"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.45", "0.1.0-linux-alpha.45"),
            Ordering::Equal
        );
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.44", "0.1.0-linux-alpha.45"),
            Ordering::Less
        );
    }

    #[test]
    fn a_release_outranks_the_prereleases_that_led_to_it() {
        assert_eq!(compare_versions("1.0.0-beta.2", "1.0.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0", "1.0.0-beta.2"), Ordering::Greater);
        assert_eq!(
            compare_versions("0.11.0-beta.0.208", "0.11.0"),
            Ordering::Less
        );
        // A higher core still wins regardless of either side's tail.
        assert_eq!(compare_versions("1.0.1-beta.1", "1.0.0"), Ordering::Greater);
        assert_eq!(
            compare_versions("1.0.0", "0.11.0-beta.2"),
            Ordering::Greater
        );

        // The pacman path: a checkout on a prerelease sees the stable release
        // the public feed advanced to, instead of reporting itself up to date.
        let update = select_deb_update_from_feed(deb_feed("1.0.0", None), "1.0.0-beta.2")
            .expect("the stable release is newer than the beta that led to it");
        assert_eq!(update.version, "1.0.0");
        assert!(
            select_deb_update_from_feed(deb_feed("1.0.0-beta.2", None), "1.0.0").is_none(),
            "a release never rolls back onto its own prerelease"
        );
    }

    #[test]
    fn prerelease_tails_compare_stage_before_counter() {
        assert_eq!(
            compare_versions("0.11.0-beta.1", "0.11.0-beta.0.109"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("0.11.0-beta.1", "0.11.0-alpha.109"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("0.11.0-beta.2.41", "0.11.0-beta.2"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0-alpha.1", "1.0.0-alpha.beta"),
            Ordering::Less,
            "a numeric identifier ranks below an alphanumeric one"
        );
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.112", "0.1.0-linux-alpha.9"),
            Ordering::Greater,
            "a hyphenated stage name is one identifier, its counter another"
        );
    }

    #[test]
    fn build_metadata_carries_no_precedence() {
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.112+7.gabc1234", "0.1.0-linux-alpha.112"),
            Ordering::Equal
        );
        assert_eq!(
            compare_versions("0.1.0-linux-alpha.113", "0.1.0-linux-alpha.112+7.gabc1234"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0+7.gabc1234", "1.0.0-beta.2"),
            Ordering::Greater,
            "metadata on a release does not turn it into a prerelease"
        );
        // A checkout seven commits past the alpha.112 tag is not offered the
        // alpha.112 package it already exceeds, and is offered alpha.113.
        assert!(
            select_deb_update_from_feed(
                deb_feed("0.1.0-linux-alpha.112", None),
                "0.1.0-linux-alpha.112+7.gabc1234"
            )
            .is_none()
        );
        assert!(
            select_deb_update_from_feed(
                deb_feed("0.1.0-linux-alpha.113", None),
                "0.1.0-linux-alpha.112+7.gabc1234"
            )
            .is_some()
        );
    }

    #[test]
    fn selects_deb_update_when_feed_is_newer_than_current() {
        let update = select_deb_update_from_feed(
            deb_feed(
                "0.1.0-linux-alpha.46",
                Some("https://example.invalid/SHA256SUMS"),
            ),
            "0.1.0-linux-alpha.45",
        )
        .expect("a newer feed should be selected");

        assert_eq!(update.version, "0.1.0-linux-alpha.46");
        assert_eq!(update.name, "ok-player_0.1.0-linux-alpha.46_amd64.deb");
        assert_eq!(
            update.url,
            "https://example.invalid/ok-player_0.1.0-linux-alpha.46_amd64.deb"
        );
        assert_eq!(update.size, Some(42));
        assert_eq!(
            update.sums_url.as_deref(),
            Some("https://example.invalid/SHA256SUMS")
        );
    }

    #[test]
    fn deb_update_leaves_sums_url_empty_when_feed_omits_manifest() {
        let update = select_deb_update_from_feed(
            deb_feed("0.1.0-linux-alpha.46", None),
            "0.1.0-linux-alpha.45",
        )
        .expect("a newer feed should be selected");

        assert!(update.sums_url.is_none());
    }

    #[test]
    fn deb_update_is_none_when_feed_is_current_or_older() {
        assert!(
            select_deb_update_from_feed(
                deb_feed("0.1.0-linux-alpha.45", None),
                "0.1.0-linux-alpha.45",
            )
            .is_none()
        );
        assert!(
            select_deb_update_from_feed(
                deb_feed("0.1.0-linux-alpha.44", None),
                "0.1.0-linux-alpha.45",
            )
            .is_none()
        );
    }

    #[test]
    fn deb_feed_parses_static_manifest_json() {
        let json = r#"{
            "version": "0.1.0-linux-alpha.108",
            "package": {
                "name": "ok-player_0.1.0-linux-alpha.108_amd64.deb",
                "url": "https://github.com/BeFeast/ok-player/releases/download/linux-v0.1.0-linux-alpha.108/ok-player_0.1.0-linux-alpha.108_amd64.deb",
                "size": 12345678
            },
            "sha256sums_url": "https://github.com/BeFeast/ok-player/releases/download/linux-v0.1.0-linux-alpha.108/SHA256SUMS"
        }"#;
        let feed: DebFeed = serde_json::from_str(json).expect("static manifest should parse");
        assert_eq!(feed.version, "0.1.0-linux-alpha.108");
        assert_eq!(feed.package.size, Some(12345678));

        let update = select_deb_update_from_feed(feed, "0.1.0-linux-alpha.107")
            .expect("alpha108 is newer than alpha107");
        assert_eq!(update.name, "ok-player_0.1.0-linux-alpha.108_amd64.deb");
        assert!(update.sums_url.is_some());
    }

    #[test]
    fn deb_feed_size_is_optional() {
        let json = r#"{
            "version": "0.1.0-linux-alpha.5",
            "package": {
                "name": "ok-player_0.1.0-linux-alpha.5_amd64.deb",
                "url": "https://example.invalid/ok-player_0.1.0-linux-alpha.5_amd64.deb"
            }
        }"#;
        let feed: DebFeed = serde_json::from_str(json).expect("size and sums are optional");
        assert!(feed.package.size.is_none());
        assert!(feed.sha256sums_url.is_none());
    }

    #[test]
    fn available_update_stays_actionable_until_the_user_acts() {
        let offer = UpdateOfferState::discovered(
            UpdateChannel::Public,
            "0.11.0-beta.2",
            &SkippedUpdateVersions::default(),
        );

        assert_eq!(offer.phase(), &UpdateOfferPhase::Available);
        assert!(offer.persistent_surface_visible());
        assert_eq!(offer.primary_action_label(), Some("Update"));
        assert!(offer.can_skip());
    }

    #[test]
    fn exact_skipped_version_is_suppressed_but_manual_install_remains_available() {
        let skipped = SkippedUpdateVersions {
            public: Some("0.11.0-beta.2".to_owned()),
            candidate: None,
        };
        let offer = UpdateOfferState::discovered(UpdateChannel::Public, "0.11.0-beta.2", &skipped);

        assert_eq!(offer.phase(), &UpdateOfferPhase::Skipped);
        assert!(!offer.persistent_surface_visible());
        assert_eq!(offer.primary_action_label(), Some("Install anyway"));
        assert!(!offer.can_skip());
    }

    #[test]
    fn newer_version_is_offered_after_the_previous_version_was_skipped() {
        let skipped = SkippedUpdateVersions {
            public: Some("0.11.0-beta.2".to_owned()),
            candidate: None,
        };
        let offer = UpdateOfferState::discovered(UpdateChannel::Public, "0.11.0-beta.3", &skipped);

        assert_eq!(offer.phase(), &UpdateOfferPhase::Available);
        assert!(offer.persistent_surface_visible());
    }

    #[test]
    fn public_and_candidate_skip_state_remains_independent() {
        let skipped = SkippedUpdateVersions {
            public: Some("0.11.0-beta.2".to_owned()),
            candidate: Some("0.11.0-beta.2.41".to_owned()),
        };

        assert!(skipped.is_skipped(UpdateChannel::Public, "0.11.0-beta.2"));
        assert!(!skipped.is_skipped(UpdateChannel::Candidate, "0.11.0-beta.2"));
        assert!(skipped.is_skipped(UpdateChannel::Candidate, "0.11.0-beta.2.41"));
        assert!(!skipped.is_skipped(UpdateChannel::Public, "0.11.0-beta.2.41"));
    }

    #[test]
    fn candidate_n_skip_does_not_suppress_candidate_n_plus_one() {
        let skipped = SkippedUpdateVersions {
            public: None,
            candidate: Some("0.11.0-beta.2.41".to_owned()),
        };
        let offer =
            UpdateOfferState::discovered(UpdateChannel::Candidate, "0.11.0-beta.2.42", &skipped);

        assert_eq!(offer.phase(), &UpdateOfferPhase::Available);
    }

    #[test]
    fn failed_install_keeps_the_same_version_retryable() {
        let mut offer = UpdateOfferState::discovered(
            UpdateChannel::Public,
            "0.11.0-beta.2",
            &SkippedUpdateVersions::default(),
        );

        assert!(offer.start_install());
        assert!(offer.install_failed("network unavailable"));
        assert_eq!(offer.version(), "0.11.0-beta.2");
        assert_eq!(
            offer.phase(),
            &UpdateOfferPhase::InstallFailed("network unavailable".to_owned())
        );
        assert!(offer.persistent_surface_visible());
        assert_eq!(offer.primary_action_label(), Some("Update"));
        assert!(offer.start_install());
    }

    #[test]
    fn successful_install_completes_the_offer() {
        let mut offer = UpdateOfferState::discovered(
            UpdateChannel::Public,
            "0.11.0-beta.2",
            &SkippedUpdateVersions::default(),
        );

        assert!(offer.start_install());
        assert!(offer.install_succeeded());
        assert_eq!(offer.phase(), &UpdateOfferPhase::Installed);
        assert!(!offer.persistent_surface_visible());
        assert_eq!(offer.primary_action_label(), None);
    }

    #[test]
    fn external_installer_handoff_keeps_the_offer_retryable() {
        let mut offer = UpdateOfferState::discovered(
            UpdateChannel::Public,
            "0.11.0-beta.2",
            &SkippedUpdateVersions::default(),
        );

        assert!(offer.start_install());
        assert!(offer.is_installing());
        assert!(offer.install_deferred());
        assert_eq!(offer.phase(), &UpdateOfferPhase::Available);
        assert!(offer.persistent_surface_visible());
        assert_eq!(offer.primary_action_label(), Some("Update"));
        assert!(offer.can_skip());
    }
}
