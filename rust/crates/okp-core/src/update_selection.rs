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

/// Orders Linux package versions by their numeric runs (`…alpha.10` after
/// `…alpha.9`), falling back to a lexicographic tiebreak for versions whose
/// numbers all match.
pub fn compare_versions(left: &str, right: &str) -> Ordering {
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
    left.cmp(right)
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
}
