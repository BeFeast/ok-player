//! Update-selection logic for the Linux self-update flow, extracted from the
//! Linux GTK shell (EPIC #134, B8): natural version comparison and choosing
//! the newest installable `.deb` from a GitHub release feed. Windows updates
//! go through Velopack's static feed, so this has no C# counterpart; the spec
//! is the shell tests that moved into this module. Network fetch, checksum
//! verification, and installation stay in the shell. See the compatibility
//! note in docs/core-compatibility.md.

use std::cmp::Ordering;

use serde::Deserialize;

/// Name of the checksum-manifest asset every Linux release publishes; the
/// selected update carries its URL so the shell can verify the download.
pub const SHA256SUMS_ASSET: &str = "SHA256SUMS";

/// Tag prefix of Linux releases (`linux-v0.1.0-linux-alpha.46`); stripping it
/// yields the package version.
const LINUX_TAG_PREFIX: &str = "linux-v";

/// A release entry of the GitHub releases feed — the fields selection reads.
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

/// A release asset of the GitHub releases feed.
#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
}

/// The `.deb` package selected from the release feed, ready for the shell to
/// download, verify against `sums_url`, and install.
#[derive(Clone, Debug)]
pub struct DebUpdate {
    pub version: String,
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
    pub sums_url: Option<String>,
}

/// Picks the newest published prerelease strictly newer than
/// `current_version` that ships an `ok-player_*_amd64.deb` asset. Drafts,
/// stable releases (the AppImage/Velopack channel), and releases without a
/// `.deb` are skipped; a missing `SHA256SUMS` asset leaves `sums_url` empty
/// for the shell to refuse.
pub fn select_latest_deb_update(
    releases: Vec<GitHubRelease>,
    current_version: &str,
) -> Option<DebUpdate> {
    let mut best = None::<DebUpdate>;
    for release in releases {
        if release.draft || !release.prerelease {
            continue;
        }
        let version = release
            .tag_name
            .strip_prefix(LINUX_TAG_PREFIX)
            .unwrap_or(&release.tag_name)
            .to_owned();
        if compare_versions(&version, current_version) != Ordering::Greater {
            continue;
        }
        let sums_url = release
            .assets
            .iter()
            .find(|asset| asset.name == SHA256SUMS_ASSET)
            .map(|asset| asset.browser_download_url.clone());
        let Some(asset) = release.assets.into_iter().find(|asset| {
            asset.name.starts_with("ok-player_") && asset.name.ends_with("_amd64.deb")
        }) else {
            continue;
        };
        let candidate = DebUpdate {
            version,
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
            sums_url,
        };
        if best.as_ref().is_none_or(|current| {
            compare_versions(&candidate.version, &current.version) == Ordering::Greater
        }) {
            best = Some(candidate);
        }
    }

    best
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

    fn github_asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: Some(42),
        }
    }

    fn github_release(
        tag_name: &str,
        draft: bool,
        prerelease: bool,
        assets: Vec<GitHubAsset>,
    ) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_owned(),
            draft,
            prerelease,
            assets,
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
    fn selects_latest_deb_prerelease_newer_than_current() {
        let update = select_latest_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.46",
                    false,
                    true,
                    vec![
                        github_asset("SHA256SUMS"),
                        github_asset("ok-player_0.1.0-linux-alpha.46_amd64.deb"),
                    ],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.47",
                    true,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.47_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.48",
                    false,
                    false,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.48_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.49",
                    false,
                    true,
                    vec![github_asset("com.befeast.okplayer.AppImage")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        )
        .expect("alpha46 .deb should be selected");

        assert_eq!(update.version, "0.1.0-linux-alpha.46");
        assert_eq!(update.name, "ok-player_0.1.0-linux-alpha.46_amd64.deb");
        assert_eq!(update.size, Some(42));
        assert_eq!(
            update.sums_url.as_deref(),
            Some("https://example.invalid/SHA256SUMS")
        );
    }

    #[test]
    fn deb_update_selection_leaves_sums_url_empty_when_release_lacks_manifest() {
        let update = select_latest_deb_update(
            vec![github_release(
                "linux-v0.1.0-linux-alpha.46",
                false,
                true,
                vec![github_asset("ok-player_0.1.0-linux-alpha.46_amd64.deb")],
            )],
            "0.1.0-linux-alpha.45",
        )
        .expect("alpha46 .deb should be selected");

        assert!(update.sums_url.is_none());
    }

    #[test]
    fn deb_update_selection_returns_none_when_only_current_or_older_exist() {
        let update = select_latest_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.44",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.44_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        );

        assert!(update.is_none());
    }
}
