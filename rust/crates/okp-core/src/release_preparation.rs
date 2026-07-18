//! Pure release-preparation models for the Linux public beta and bounded
//! historical GitHub Release cleanup (issue #350).
//!
//! Network collection and file I/O live in the companion CLI. This module
//! owns the archive schema, exact retain allowlist, non-mutating cleanup plan,
//! migration-anchor availability record, and public update-feed audit so the
//! safety rules are testable without a shell or GitHub connection.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::acceptance_evidence::{ArtifactKind, PackageIdentity};
use crate::sha256sums::sha256_hex;
use crate::update_selection::{DebFeed, compare_versions};

pub const RELEASE_ARCHIVE_SCHEMA_VERSION: u32 = 1;
pub const RELEASE_RETAIN_ALLOWLIST_SCHEMA_VERSION: u32 = 1;
pub const RELEASE_CLEANUP_PLAN_SCHEMA_VERSION: u32 = 1;
pub const MIGRATION_ANCHOR_CHECK_SCHEMA_VERSION: u32 = 1;
pub const PUBLIC_FEED_AUDIT_SCHEMA_VERSION: u32 = 1;
pub const PUBLIC_FEED_COMPARISON_SCHEMA_VERSION: u32 = 1;
pub const LINUX_RELEASE_TAG_PREFIX: &str = "linux-v";

pub fn validate_public_beta_release_notes(
    notes: &str,
    identity: &PackageIdentity,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if notes.trim().is_empty() {
        errors.push("release notes are empty".to_owned());
        return Err(errors);
    }
    if notes.contains('⟨') || notes.contains('⟩') || notes.contains("**Template.**") {
        errors.push("release notes still contain template-only text or placeholders".to_owned());
    }
    for heading in [
        "## User-visible changes",
        "## Supported distro / session / package matrix",
        "## Install, update, rollback, and uninstall",
        "## Checksums, source SHA, and provenance",
        "## Known limitations",
        "## Acceptance summary",
    ] {
        if !notes.contains(heading) {
            errors.push(format!(
                "release notes are missing required section: {heading}"
            ));
        }
    }
    if !notes.contains(&identity.version) {
        errors.push(format!(
            "release notes do not name package version {}",
            identity.version
        ));
    }
    if !notes.contains(&identity.commit_sha) {
        errors.push(format!(
            "release notes do not contain exact source SHA {}",
            identity.commit_sha
        ));
    }
    for kind in [ArtifactKind::Debian, ArtifactKind::AppImage] {
        let matching = identity
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == kind)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            errors.push(format!(
                "package identity must contain exactly one {kind:?} artifact"
            ));
            continue;
        }
        let artifact = matching[0];
        if !notes.contains(&artifact.file_name) {
            errors.push(format!(
                "release notes do not name artifact {}",
                artifact.file_name
            ));
        }
        if !notes
            .to_ascii_lowercase()
            .contains(&artifact.sha256.to_ascii_lowercase())
        {
            errors.push(format!(
                "release notes do not contain SHA-256 for {}",
                artifact.file_name
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubReleaseInput {
    pub id: u64,
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub target_commitish: String,
    #[serde(default)]
    pub assets: Vec<GitHubReleaseAssetInput>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitHubReleaseAssetInput {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedGitTag {
    pub tag_name: String,
    pub ref_sha: String,
    pub source_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LinuxReleaseArchive {
    pub schema_version: u32,
    pub repository: String,
    pub generated_at_unix: u64,
    pub release_tag_prefix: String,
    pub summary: ReleaseArchiveSummary,
    pub release_objects: Vec<ArchivedRelease>,
    pub git_tags: Vec<ArchivedGitTag>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseArchiveSummary {
    pub release_object_count: usize,
    pub git_tag_count: usize,
    pub release_asset_count: usize,
    pub tags_without_release_objects: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchivedRelease {
    pub id: u64,
    pub tag_name: String,
    pub source_sha: String,
    pub target_commitish: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub assets: Vec<ArchivedReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchivedReleaseAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub browser_download_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchivedGitTag {
    pub tag_name: String,
    pub ref_sha: String,
    pub source_sha: String,
    pub has_release_object: bool,
}

pub fn build_linux_release_archive(
    repository: &str,
    generated_at_unix: u64,
    releases: Vec<GitHubReleaseInput>,
    tags: Vec<ResolvedGitTag>,
) -> Result<LinuxReleaseArchive, Vec<String>> {
    let mut errors = Vec::new();
    validate_repository(repository, &mut errors);

    let mut tags_by_name = BTreeMap::new();
    for tag in tags {
        if !tag.tag_name.starts_with(LINUX_RELEASE_TAG_PREFIX) {
            continue;
        }
        validate_tag_name(&tag.tag_name, &mut errors);
        validate_git_sha("tag ref SHA", &tag.ref_sha, &mut errors);
        validate_git_sha("tag source SHA", &tag.source_sha, &mut errors);
        let name = tag.tag_name.clone();
        if tags_by_name.insert(name.clone(), tag).is_some() {
            errors.push(format!("duplicate git tag in archive input: {name}"));
        }
    }

    let mut release_ids = BTreeSet::new();
    let mut release_tags = BTreeSet::new();
    let mut archived_releases = Vec::new();
    for release in releases
        .into_iter()
        .filter(|release| release.tag_name.starts_with(LINUX_RELEASE_TAG_PREFIX))
    {
        if !release_ids.insert(release.id) {
            errors.push(format!("duplicate release object id: {}", release.id));
        }
        if !release_tags.insert(release.tag_name.clone()) {
            errors.push(format!(
                "duplicate Linux release object tag: {}",
                release.tag_name
            ));
        }
        validate_tag_name(&release.tag_name, &mut errors);
        let Some(tag) = tags_by_name.get(&release.tag_name) else {
            errors.push(format!(
                "Linux release object {} has no matching git tag",
                release.tag_name
            ));
            continue;
        };
        if is_git_sha(&release.target_commitish)
            && !release
                .target_commitish
                .eq_ignore_ascii_case(&tag.source_sha)
        {
            errors.push(format!(
                "release {} target_commitish {} does not match tag source SHA {}",
                release.tag_name, release.target_commitish, tag.source_sha
            ));
        }

        let mut asset_ids = BTreeSet::new();
        let mut asset_names = BTreeSet::new();
        let mut assets = Vec::new();
        for asset in release.assets {
            if !asset_ids.insert(asset.id) {
                errors.push(format!(
                    "release {} contains duplicate asset id {}",
                    release.tag_name, asset.id
                ));
            }
            if !asset_names.insert(asset.name.clone()) {
                errors.push(format!(
                    "release {} contains duplicate asset name {}",
                    release.tag_name, asset.name
                ));
            }
            let sha256 = match asset.digest.as_deref() {
                Some(digest) => match digest.strip_prefix("sha256:") {
                    Some(value) if is_sha256(value) => value.to_ascii_lowercase(),
                    _ => {
                        errors.push(format!(
                            "release {} asset {} has no valid GitHub SHA-256 digest",
                            release.tag_name, asset.name
                        ));
                        continue;
                    }
                },
                None => {
                    errors.push(format!(
                        "release {} asset {} has no GitHub digest",
                        release.tag_name, asset.name
                    ));
                    continue;
                }
            };
            validate_release_asset_url(
                repository,
                &release.tag_name,
                &asset.name,
                &asset.browser_download_url,
                &mut errors,
            );
            assets.push(ArchivedReleaseAsset {
                id: asset.id,
                name: asset.name,
                size: asset.size,
                sha256,
                browser_download_url: asset.browser_download_url,
                created_at: asset.created_at,
                updated_at: asset.updated_at,
            });
        }
        assets.sort_by(|left, right| left.name.cmp(&right.name));
        archived_releases.push(ArchivedRelease {
            id: release.id,
            tag_name: release.tag_name,
            source_sha: tag.source_sha.clone(),
            target_commitish: release.target_commitish,
            name: release.name,
            body: release.body,
            html_url: release.html_url,
            draft: release.draft,
            prerelease: release.prerelease,
            created_at: release.created_at,
            published_at: release.published_at,
            assets,
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    archived_releases.sort_by(|left, right| {
        left.published_at
            .cmp(&right.published_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut archived_tags = tags_by_name
        .into_values()
        .map(|tag| ArchivedGitTag {
            has_release_object: release_tags.contains(&tag.tag_name),
            tag_name: tag.tag_name,
            ref_sha: tag.ref_sha,
            source_sha: tag.source_sha,
        })
        .collect::<Vec<_>>();
    archived_tags.sort_by(|left, right| left.tag_name.cmp(&right.tag_name));
    let release_asset_count = archived_releases
        .iter()
        .map(|release| release.assets.len())
        .sum();
    let tags_without_release_objects = archived_tags
        .iter()
        .filter(|tag| !tag.has_release_object)
        .count();

    Ok(LinuxReleaseArchive {
        schema_version: RELEASE_ARCHIVE_SCHEMA_VERSION,
        repository: repository.to_owned(),
        generated_at_unix,
        release_tag_prefix: LINUX_RELEASE_TAG_PREFIX.to_owned(),
        summary: ReleaseArchiveSummary {
            release_object_count: archived_releases.len(),
            git_tag_count: archived_tags.len(),
            release_asset_count,
            tags_without_release_objects,
        },
        release_objects: archived_releases,
        git_tags: archived_tags,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseRetainAllowlist {
    pub schema_version: u32,
    pub repository: String,
    pub entries: Vec<ReleaseRetainEntry>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseRetainKind {
    MigrationAnchor,
    CurrentPublicRelease,
    ExplicitException,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseRetainEntry {
    pub tag_name: String,
    pub kind: ReleaseRetainKind,
    pub reason: String,
    pub required_in_archive: bool,
    pub removal_gate: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleaseCleanupPlan {
    pub schema_version: u32,
    pub repository: String,
    pub archive_generated_at_unix: u64,
    pub dry_run: bool,
    pub execution_ready: bool,
    pub execution_blockers: Vec<String>,
    pub migration_evidence_validated: bool,
    pub batch_size: usize,
    pub summary: ReleaseCleanupSummary,
    pub exact_retain_allowlist: Vec<ReleaseRetainEntry>,
    pub retained_release_objects: Vec<CleanupReleaseObject>,
    pub allowlisted_tags_not_yet_released: Vec<String>,
    pub batches: Vec<ReleaseCleanupBatch>,
    pub preserved_git_tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleaseCleanupSummary {
    pub archived_release_objects: usize,
    pub retained_release_objects: usize,
    pub planned_release_object_deletions: usize,
    pub preserved_git_tags: usize,
    pub batch_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CleanupReleaseObject {
    pub id: u64,
    pub tag_name: String,
    pub source_sha: String,
    pub published_at: Option<String>,
    pub html_url: String,
    pub release_api_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReleaseCleanupBatch {
    pub number: usize,
    pub release_objects: Vec<CleanupReleaseObject>,
}

pub fn plan_linux_release_cleanup(
    archive: &LinuxReleaseArchive,
    allowlist: &ReleaseRetainAllowlist,
    batch_size: usize,
    migration_evidence_validated: bool,
) -> Result<ReleaseCleanupPlan, Vec<String>> {
    let mut errors = Vec::new();
    validate_archive_header(archive, &mut errors);
    if allowlist.schema_version != RELEASE_RETAIN_ALLOWLIST_SCHEMA_VERSION {
        errors.push(format!(
            "unsupported retain allowlist schema {}, expected {}",
            allowlist.schema_version, RELEASE_RETAIN_ALLOWLIST_SCHEMA_VERSION
        ));
    }
    if allowlist.repository != archive.repository {
        errors.push(format!(
            "retain allowlist repository {} does not match archive repository {}",
            allowlist.repository, archive.repository
        ));
    }
    if batch_size == 0 {
        errors.push("cleanup batch size must be greater than zero".to_owned());
    }

    let release_tags = archive
        .release_objects
        .iter()
        .map(|release| release.tag_name.as_str())
        .collect::<BTreeSet<_>>();
    let mut retain_tags = BTreeSet::new();
    let mut has_migration_anchor = false;
    for entry in &allowlist.entries {
        validate_tag_name(&entry.tag_name, &mut errors);
        if entry.reason.trim().is_empty() {
            errors.push(format!(
                "retain entry {} has an empty reason",
                entry.tag_name
            ));
        }
        if entry.removal_gate.trim().is_empty() {
            errors.push(format!(
                "retain entry {} has an empty removal gate",
                entry.tag_name
            ));
        }
        if !retain_tags.insert(entry.tag_name.as_str()) {
            errors.push(format!(
                "duplicate retain allowlist tag: {}",
                entry.tag_name
            ));
        }
        if entry.kind == ReleaseRetainKind::MigrationAnchor {
            has_migration_anchor = true;
            if !entry.required_in_archive {
                errors.push(format!(
                    "migration anchor {} must be required in the archive",
                    entry.tag_name
                ));
            }
        }
        if entry.required_in_archive && !release_tags.contains(entry.tag_name.as_str()) {
            errors.push(format!(
                "required retained release {} is missing from the archive",
                entry.tag_name
            ));
        }
    }
    if !has_migration_anchor {
        errors.push("retain allowlist must name at least one migration anchor".to_owned());
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let to_cleanup_object = |release: &ArchivedRelease| CleanupReleaseObject {
        id: release.id,
        tag_name: release.tag_name.clone(),
        source_sha: release.source_sha.clone(),
        published_at: release.published_at.clone(),
        html_url: release.html_url.clone(),
        release_api_path: format!("repos/{}/releases/{}", archive.repository, release.id),
    };
    let retained_release_objects = archive
        .release_objects
        .iter()
        .filter(|release| retain_tags.contains(release.tag_name.as_str()))
        .map(to_cleanup_object)
        .collect::<Vec<_>>();
    let deletions = archive
        .release_objects
        .iter()
        .filter(|release| !retain_tags.contains(release.tag_name.as_str()))
        .map(to_cleanup_object)
        .collect::<Vec<_>>();
    let batches = deletions
        .chunks(batch_size)
        .enumerate()
        .map(|(index, release_objects)| ReleaseCleanupBatch {
            number: index + 1,
            release_objects: release_objects.to_vec(),
        })
        .collect::<Vec<_>>();
    let allowlisted_tags_not_yet_released = allowlist
        .entries
        .iter()
        .filter(|entry| !release_tags.contains(entry.tag_name.as_str()))
        .map(|entry| entry.tag_name.clone())
        .collect::<Vec<_>>();
    let mut execution_blockers = Vec::new();
    if !migration_evidence_validated {
        execution_blockers
            .push("candidate-upgrade migration evidence was not supplied and validated".to_owned());
    }
    for entry in &allowlist.entries {
        if entry.kind == ReleaseRetainKind::CurrentPublicRelease
            && !release_tags.contains(entry.tag_name.as_str())
        {
            execution_blockers.push(format!(
                "current public release {} is not present in the archive",
                entry.tag_name
            ));
        }
    }
    let preserved_git_tags = archive
        .git_tags
        .iter()
        .map(|tag| tag.tag_name.clone())
        .collect::<Vec<_>>();

    Ok(ReleaseCleanupPlan {
        schema_version: RELEASE_CLEANUP_PLAN_SCHEMA_VERSION,
        repository: archive.repository.clone(),
        archive_generated_at_unix: archive.generated_at_unix,
        dry_run: true,
        execution_ready: execution_blockers.is_empty(),
        execution_blockers,
        migration_evidence_validated,
        batch_size,
        summary: ReleaseCleanupSummary {
            archived_release_objects: archive.release_objects.len(),
            retained_release_objects: retained_release_objects.len(),
            planned_release_object_deletions: deletions.len(),
            preserved_git_tags: preserved_git_tags.len(),
            batch_count: batches.len(),
        },
        exact_retain_allowlist: allowlist.entries.clone(),
        retained_release_objects,
        allowlisted_tags_not_yet_released,
        batches,
        preserved_git_tags,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MigrationAnchorCheck {
    pub schema_version: u32,
    pub repository: String,
    pub tag_name: String,
    pub source_sha: String,
    pub checked_at_unix: u64,
    pub release_downloadable: bool,
    pub assets: Vec<AssetAvailability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssetAvailability {
    pub name: String,
    pub sha256: String,
    pub browser_download_url: String,
    pub http_status: u16,
    pub downloadable: bool,
}

pub fn migration_anchor_check(
    archive: &LinuxReleaseArchive,
    tag_name: &str,
    checked_at_unix: u64,
    http_statuses: &BTreeMap<String, u16>,
) -> Result<MigrationAnchorCheck, Vec<String>> {
    let mut errors = Vec::new();
    validate_archive_header(archive, &mut errors);
    let Some(release) = archive
        .release_objects
        .iter()
        .find(|release| release.tag_name == tag_name)
    else {
        errors.push(format!(
            "migration anchor {tag_name} is missing from archive"
        ));
        return Err(errors);
    };
    if release.assets.is_empty() {
        errors.push(format!("migration anchor {tag_name} contains no assets"));
    }
    let mut assets = Vec::new();
    for asset in &release.assets {
        let Some(status) = http_statuses.get(&asset.browser_download_url).copied() else {
            errors.push(format!(
                "migration anchor asset {} has no availability result",
                asset.name
            ));
            continue;
        };
        assets.push(AssetAvailability {
            name: asset.name.clone(),
            sha256: asset.sha256.clone(),
            browser_download_url: asset.browser_download_url.clone(),
            http_status: status,
            downloadable: (200..400).contains(&status),
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let release_downloadable = assets.iter().all(|asset| asset.downloadable);
    if !release_downloadable {
        return Err(assets
            .iter()
            .filter(|asset| !asset.downloadable)
            .map(|asset| {
                format!(
                    "migration anchor asset {} returned HTTP {}",
                    asset.name, asset.http_status
                )
            })
            .collect());
    }
    Ok(MigrationAnchorCheck {
        schema_version: MIGRATION_ANCHOR_CHECK_SCHEMA_VERSION,
        repository: archive.repository.clone(),
        tag_name: release.tag_name.clone(),
        source_sha: release.source_sha.clone(),
        checked_at_unix,
        release_downloadable,
        assets,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublicFeedAudit {
    pub schema_version: u32,
    pub repository: String,
    pub checked_at_unix: u64,
    pub linux: LinuxFeedAudit,
    pub windows: WindowsFeedAudit,
    pub referenced_assets: Vec<FeedAssetAvailability>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LinuxFeedAudit {
    pub expected_version: String,
    pub installed_version: String,
    pub update_offered: bool,
    pub deb_feed_sha256: String,
    pub velopack_feed_sha256: String,
    pub deb_package_url: String,
    pub sha256sums_url: String,
    pub velopack_full_package_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowsFeedAudit {
    pub expected_version: String,
    pub installed_version: String,
    pub update_offered: bool,
    pub feed_sha256: String,
    pub full_package_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedAssetAvailability {
    pub url: String,
    pub http_status: u16,
    pub downloadable: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeed {
    #[serde(rename = "Assets")]
    assets: Vec<VelopackFeedAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeedAsset {
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FileName")]
    file_name: String,
    #[serde(rename = "SHA256", default)]
    sha256: Option<String>,
    #[serde(rename = "Size", default)]
    size: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct PublicFeedAuditInput<'a> {
    pub repository: &'a str,
    pub checked_at_unix: u64,
    pub expected_linux_version: &'a str,
    pub installed_linux_version: &'a str,
    pub expected_windows_version: &'a str,
    pub installed_windows_version: &'a str,
    pub linux_deb_feed: &'a [u8],
    pub linux_velopack_feed: &'a [u8],
    pub windows_feed: &'a [u8],
    pub http_statuses: &'a BTreeMap<String, u16>,
}

pub fn public_feed_asset_urls(
    linux_deb_feed: &[u8],
    linux_velopack_feed: &[u8],
    windows_feed: &[u8],
) -> Result<Vec<String>, Vec<String>> {
    let (deb_feed, linux_velopack, windows) =
        parse_public_feeds(linux_deb_feed, linux_velopack_feed, windows_feed)?;
    let mut urls = BTreeSet::new();
    urls.insert(deb_feed.package.url);
    if let Some(url) = deb_feed.sha256sums_url {
        urls.insert(url);
    }
    for asset in linux_velopack.assets.iter().chain(windows.assets.iter()) {
        urls.insert(asset.file_name.clone());
    }
    Ok(urls.into_iter().collect())
}

pub fn audit_public_update_feeds(
    input: PublicFeedAuditInput<'_>,
) -> Result<PublicFeedAudit, Vec<String>> {
    let mut errors = Vec::new();
    validate_repository(input.repository, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let (deb_feed, linux_velopack, windows) = parse_public_feeds(
        input.linux_deb_feed,
        input.linux_velopack_feed,
        input.windows_feed,
    )?;

    if deb_feed.version != input.expected_linux_version {
        errors.push(format!(
            "Linux .deb feed version {} does not match expected {}",
            deb_feed.version, input.expected_linux_version
        ));
    }
    let linux_tag = format!("linux-v{}", input.expected_linux_version);
    validate_feed_asset_url(
        input.repository,
        &linux_tag,
        &deb_feed.package.url,
        &mut errors,
    );
    if !deb_feed.package.url.ends_with(&deb_feed.package.name) {
        errors.push("Linux .deb feed package name does not match its URL".to_owned());
    }
    let sha256sums_url = deb_feed.sha256sums_url.clone().unwrap_or_default();
    if sha256sums_url.is_empty() {
        errors.push("Linux .deb feed has no SHA256SUMS URL".to_owned());
    } else {
        validate_feed_asset_url(input.repository, &linux_tag, &sha256sums_url, &mut errors);
        if !sha256sums_url.ends_with("/SHA256SUMS") {
            errors.push("Linux .deb feed checksum URL does not name SHA256SUMS".to_owned());
        }
    }

    let linux_full = validate_velopack_feed(
        "Linux",
        input.repository,
        &linux_tag,
        input.expected_linux_version,
        &linux_velopack,
        true,
        &mut errors,
    );
    let windows_tag = format!("v{}", input.expected_windows_version);
    let windows_full = validate_velopack_feed(
        "Windows",
        input.repository,
        &windows_tag,
        input.expected_windows_version,
        &windows,
        false,
        &mut errors,
    );

    let mut referenced_urls = BTreeSet::new();
    referenced_urls.insert(deb_feed.package.url.clone());
    if !sha256sums_url.is_empty() {
        referenced_urls.insert(sha256sums_url.clone());
    }
    for asset in linux_velopack.assets.iter().chain(windows.assets.iter()) {
        referenced_urls.insert(asset.file_name.clone());
    }
    let mut referenced_assets = Vec::new();
    for url in referenced_urls {
        match input.http_statuses.get(&url).copied() {
            Some(http_status) => {
                let downloadable = (200..400).contains(&http_status);
                if !downloadable {
                    errors.push(format!(
                        "update-feed asset {url} returned HTTP {http_status}"
                    ));
                }
                referenced_assets.push(FeedAssetAvailability {
                    url,
                    http_status,
                    downloadable,
                });
            }
            None => errors.push(format!(
                "update-feed asset {url} was not availability-checked"
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let Some(linux_full) = linux_full else {
        return Err(vec![
            "Linux Velopack feed contains no validated full package".to_owned(),
        ]);
    };
    let Some(windows_full) = windows_full else {
        return Err(vec![
            "Windows Velopack feed contains no validated full package".to_owned(),
        ]);
    };

    Ok(PublicFeedAudit {
        schema_version: PUBLIC_FEED_AUDIT_SCHEMA_VERSION,
        repository: input.repository.to_owned(),
        checked_at_unix: input.checked_at_unix,
        linux: LinuxFeedAudit {
            expected_version: input.expected_linux_version.to_owned(),
            installed_version: input.installed_linux_version.to_owned(),
            update_offered: compare_versions(
                input.expected_linux_version,
                input.installed_linux_version,
            )
            .is_gt(),
            deb_feed_sha256: sha256_hex(input.linux_deb_feed),
            velopack_feed_sha256: sha256_hex(input.linux_velopack_feed),
            deb_package_url: deb_feed.package.url,
            sha256sums_url,
            velopack_full_package_url: linux_full,
        },
        windows: WindowsFeedAudit {
            expected_version: input.expected_windows_version.to_owned(),
            installed_version: input.installed_windows_version.to_owned(),
            update_offered: compare_versions(
                input.expected_windows_version,
                input.installed_windows_version,
            )
            .is_gt(),
            feed_sha256: sha256_hex(input.windows_feed),
            full_package_url: windows_full,
        },
        referenced_assets,
    })
}

fn parse_public_feeds(
    linux_deb_feed: &[u8],
    linux_velopack_feed: &[u8],
    windows_feed: &[u8],
) -> Result<(DebFeed, VelopackFeed, VelopackFeed), Vec<String>> {
    let mut errors = Vec::new();
    let deb_feed = match serde_json::from_slice::<DebFeed>(linux_deb_feed) {
        Ok(feed) => Some(feed),
        Err(error) => {
            errors.push(format!("invalid Linux .deb feed: {error}"));
            None
        }
    };
    let linux_velopack = match serde_json::from_slice::<VelopackFeed>(linux_velopack_feed) {
        Ok(feed) => Some(feed),
        Err(error) => {
            errors.push(format!("invalid Linux Velopack feed: {error}"));
            None
        }
    };
    let windows = match serde_json::from_slice::<VelopackFeed>(windows_feed) {
        Ok(feed) => Some(feed),
        Err(error) => {
            errors.push(format!("invalid Windows Velopack feed: {error}"));
            None
        }
    };
    match (deb_feed, linux_velopack, windows) {
        (Some(deb_feed), Some(linux_velopack), Some(windows)) if errors.is_empty() => {
            Ok((deb_feed, linux_velopack, windows))
        }
        _ => Err(errors),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicFeedComparison {
    pub schema_version: u32,
    pub repository: String,
    pub before_checked_at_unix: u64,
    pub after_checked_at_unix: u64,
    pub unchanged: bool,
    pub linux_deb_feed_sha256: String,
    pub linux_velopack_feed_sha256: String,
    pub windows_feed_sha256: String,
}

pub fn compare_public_feed_audits(
    before: &PublicFeedAudit,
    after: &PublicFeedAudit,
) -> Result<PublicFeedComparison, Vec<String>> {
    let mut errors = Vec::new();
    for (label, audit) in [("before", before), ("after", after)] {
        if audit.schema_version != PUBLIC_FEED_AUDIT_SCHEMA_VERSION {
            errors.push(format!(
                "{label} feed audit schema {} does not match {}",
                audit.schema_version, PUBLIC_FEED_AUDIT_SCHEMA_VERSION
            ));
        }
    }
    if before.repository != after.repository {
        errors.push(format!(
            "feed audit repositories differ: {} vs {}",
            before.repository, after.repository
        ));
    }
    if before.linux != after.linux {
        errors.push("Linux public feed audit changed across the cleanup batch".to_owned());
    }
    if before.windows != after.windows {
        errors.push("Windows public feed audit changed across the cleanup batch".to_owned());
    }
    if before.referenced_assets != after.referenced_assets {
        errors.push(
            "referenced public feed asset availability changed across the cleanup batch".to_owned(),
        );
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(PublicFeedComparison {
        schema_version: PUBLIC_FEED_COMPARISON_SCHEMA_VERSION,
        repository: before.repository.clone(),
        before_checked_at_unix: before.checked_at_unix,
        after_checked_at_unix: after.checked_at_unix,
        unchanged: true,
        linux_deb_feed_sha256: before.linux.deb_feed_sha256.clone(),
        linux_velopack_feed_sha256: before.linux.velopack_feed_sha256.clone(),
        windows_feed_sha256: before.windows.feed_sha256.clone(),
    })
}

fn validate_velopack_feed(
    label: &str,
    repository: &str,
    tag_name: &str,
    expected_version: &str,
    feed: &VelopackFeed,
    require_sha256: bool,
    errors: &mut Vec<String>,
) -> Option<String> {
    if feed.assets.is_empty() {
        errors.push(format!("{label} Velopack feed contains no assets"));
        return None;
    }
    let mut full_url = None;
    for asset in &feed.assets {
        if asset.version != expected_version {
            errors.push(format!(
                "{label} Velopack asset version {} does not match expected {}",
                asset.version, expected_version
            ));
        }
        validate_feed_asset_url(repository, tag_name, &asset.file_name, errors);
        if require_sha256 {
            match asset.sha256.as_deref() {
                Some(value) if is_sha256(value) => {}
                _ => errors.push(format!(
                    "{label} Velopack asset {} has no valid SHA-256",
                    asset.file_name
                )),
            }
            if asset.size == Some(0) || asset.size.is_none() {
                errors.push(format!(
                    "{label} Velopack asset {} has no positive size",
                    asset.file_name
                ));
            }
        }
        if asset.kind.eq_ignore_ascii_case("full")
            && full_url.replace(asset.file_name.clone()).is_some()
        {
            errors.push(format!(
                "{label} Velopack feed contains more than one full package"
            ));
        }
    }
    if full_url.is_none() {
        errors.push(format!("{label} Velopack feed contains no full package"));
    }
    full_url
}

fn validate_archive_header(archive: &LinuxReleaseArchive, errors: &mut Vec<String>) {
    if archive.schema_version != RELEASE_ARCHIVE_SCHEMA_VERSION {
        errors.push(format!(
            "unsupported release archive schema {}, expected {}",
            archive.schema_version, RELEASE_ARCHIVE_SCHEMA_VERSION
        ));
    }
    validate_repository(&archive.repository, errors);
    if archive.release_tag_prefix != LINUX_RELEASE_TAG_PREFIX {
        errors.push(format!(
            "release archive prefix {} does not match {}",
            archive.release_tag_prefix, LINUX_RELEASE_TAG_PREFIX
        ));
    }
}

fn validate_repository(repository: &str, errors: &mut Vec<String>) {
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        errors.push(format!(
            "repository must be an owner/name pair, got {repository}"
        ));
    }
}

fn validate_tag_name(tag_name: &str, errors: &mut Vec<String>) {
    if !tag_name.starts_with(LINUX_RELEASE_TAG_PREFIX)
        || tag_name.len() == LINUX_RELEASE_TAG_PREFIX.len()
    {
        errors.push(format!("invalid Linux release tag: {tag_name}"));
    }
}

fn validate_git_sha(label: &str, value: &str, errors: &mut Vec<String>) {
    if !is_git_sha(value) {
        errors.push(format!(
            "{label} is not a 40-character hexadecimal SHA: {value}"
        ));
    }
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_release_asset_url(
    repository: &str,
    tag_name: &str,
    asset_name: &str,
    url: &str,
    errors: &mut Vec<String>,
) {
    let expected =
        format!("https://github.com/{repository}/releases/download/{tag_name}/{asset_name}");
    if url != expected {
        errors.push(format!(
            "release asset URL does not match its repository/tag/name: {url}"
        ));
    }
}

fn validate_feed_asset_url(repository: &str, tag_name: &str, url: &str, errors: &mut Vec<String>) {
    let expected_prefix = format!("https://github.com/{repository}/releases/download/{tag_name}/");
    if !url.starts_with(&expected_prefix) || url.len() == expected_prefix.len() {
        errors.push(format!(
            "feed asset URL is not attached to {tag_name}: {url}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn release(id: u64, tag_name: &str, sha: &str) -> GitHubReleaseInput {
        let asset_name = format!("ok-player_{}_amd64.deb", &tag_name[7..]);
        GitHubReleaseInput {
            id,
            tag_name: tag_name.to_owned(),
            name: Some(tag_name.to_owned()),
            body: Some("release notes".to_owned()),
            html_url: format!("https://github.com/acme/player/releases/tag/{tag_name}"),
            draft: false,
            prerelease: true,
            created_at: format!("2026-01-{id:02}T00:00:00Z"),
            published_at: Some(format!("2026-01-{id:02}T00:00:00Z")),
            target_commitish: sha.to_owned(),
            assets: vec![GitHubReleaseAssetInput {
                id: id * 10,
                name: asset_name.clone(),
                size: 42,
                browser_download_url: format!(
                    "https://github.com/acme/player/releases/download/{tag_name}/{asset_name}"
                ),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:01Z".to_owned(),
                digest: Some(format!("sha256:{DIGEST_A}")),
            }],
        }
    }

    fn tag(tag_name: &str, sha: &str) -> ResolvedGitTag {
        ResolvedGitTag {
            tag_name: tag_name.to_owned(),
            ref_sha: sha.to_owned(),
            source_sha: sha.to_owned(),
        }
    }

    fn archive() -> LinuxReleaseArchive {
        build_linux_release_archive(
            "acme/player",
            42,
            vec![
                release(1, "linux-v0.1.0-alpha.1", SHA_A),
                release(2, "linux-v0.1.0-alpha.2", SHA_B),
            ],
            vec![
                tag("linux-v0.1.0-alpha.1", SHA_A),
                tag("linux-v0.1.0-alpha.2", SHA_B),
                tag("linux-v0.1.0-alpha.3", SHA_A),
            ],
        )
        .expect("fixture archive should build")
    }

    #[test]
    fn archive_maps_release_objects_to_source_tags_assets_and_checksums() {
        let archive = archive();
        assert_eq!(archive.summary.release_object_count, 2);
        assert_eq!(archive.summary.git_tag_count, 3);
        assert_eq!(archive.summary.release_asset_count, 2);
        assert_eq!(archive.summary.tags_without_release_objects, 1);
        assert_eq!(archive.release_objects[0].source_sha, SHA_A);
        assert_eq!(archive.release_objects[0].assets[0].sha256, DIGEST_A);
        assert!(
            archive
                .git_tags
                .iter()
                .any(|tag| tag.tag_name == "linux-v0.1.0-alpha.3" && !tag.has_release_object)
        );
    }

    #[test]
    fn archive_rejects_missing_tags_and_unverifiable_assets() {
        let mut missing_digest = release(1, "linux-v0.1.0-alpha.1", SHA_A);
        missing_digest.assets[0].digest = None;
        let errors = build_linux_release_archive(
            "acme/player",
            42,
            vec![missing_digest, release(2, "linux-v0.1.0-alpha.2", SHA_B)],
            vec![tag("linux-v0.1.0-alpha.1", SHA_A)],
        )
        .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("has no GitHub digest"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("has no matching git tag"))
        );
    }

    fn allowlist() -> ReleaseRetainAllowlist {
        ReleaseRetainAllowlist {
            schema_version: RELEASE_RETAIN_ALLOWLIST_SCHEMA_VERSION,
            repository: "acme/player".to_owned(),
            entries: vec![
                ReleaseRetainEntry {
                    tag_name: "linux-v0.1.0-alpha.2".to_owned(),
                    kind: ReleaseRetainKind::MigrationAnchor,
                    reason: "installed predecessor".to_owned(),
                    required_in_archive: true,
                    removal_gate: "migration window closed".to_owned(),
                },
                ReleaseRetainEntry {
                    tag_name: "linux-v0.11.0-beta.1".to_owned(),
                    kind: ReleaseRetainKind::CurrentPublicRelease,
                    reason: "public beta".to_owned(),
                    required_in_archive: false,
                    removal_gate: "superseded deliberately".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn cleanup_plan_is_dry_run_bounded_and_preserves_every_git_tag() {
        let plan = plan_linux_release_cleanup(&archive(), &allowlist(), 1, false)
            .expect("cleanup plan should validate");
        assert!(plan.dry_run);
        assert!(!plan.execution_ready);
        assert_eq!(plan.execution_blockers.len(), 2);
        assert_eq!(plan.summary.planned_release_object_deletions, 1);
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(
            plan.batches[0].release_objects[0].tag_name,
            "linux-v0.1.0-alpha.1"
        );
        assert_eq!(
            plan.retained_release_objects[0].tag_name,
            "linux-v0.1.0-alpha.2"
        );
        assert_eq!(plan.preserved_git_tags.len(), 3);
        assert_eq!(
            plan.allowlisted_tags_not_yet_released,
            ["linux-v0.11.0-beta.1"]
        );
    }

    #[test]
    fn cleanup_plan_requires_an_archived_migration_anchor() {
        let mut allowlist = allowlist();
        allowlist.entries[0].tag_name = "linux-v0.1.0-alpha.99".to_owned();
        let errors = plan_linux_release_cleanup(&archive(), &allowlist, 10, true).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("required retained release"))
        );
    }

    #[test]
    fn migration_anchor_check_requires_every_archived_asset_to_download() {
        let archive = archive();
        let asset = &archive.release_objects[1].assets[0];
        let statuses = BTreeMap::from([(asset.browser_download_url.clone(), 200)]);
        let check = migration_anchor_check(&archive, "linux-v0.1.0-alpha.2", 43, &statuses)
            .expect("anchor should be downloadable");
        assert!(check.release_downloadable);

        let failed = BTreeMap::from([(asset.browser_download_url.clone(), 404)]);
        assert!(
            migration_anchor_check(&archive, "linux-v0.1.0-alpha.2", 43, &failed).unwrap_err()[0]
                .contains("HTTP 404")
        );
    }

    fn feed_fixture() -> (Vec<u8>, Vec<u8>, Vec<u8>, BTreeMap<String, u16>) {
        let linux_deb_url = "https://github.com/acme/player/releases/download/linux-v0.11.0-beta.1/ok-player_0.11.0-beta.1_amd64.deb";
        let sums_url =
            "https://github.com/acme/player/releases/download/linux-v0.11.0-beta.1/SHA256SUMS";
        let linux_full = "https://github.com/acme/player/releases/download/linux-v0.11.0-beta.1/com.acme.player-0.11.0-beta.1-linux-full.nupkg";
        let windows_full =
            "https://github.com/acme/player/releases/download/v0.10.14/Player-0.10.14-full.nupkg";
        let linux_deb = format!(
            r#"{{"version":"0.11.0-beta.1","package":{{"name":"ok-player_0.11.0-beta.1_amd64.deb","url":"{linux_deb_url}","size":42}},"sha256sums_url":"{sums_url}"}}"#
        )
        .into_bytes();
        let linux_velopack = format!(
            r#"{{"Assets":[{{"Version":"0.11.0-beta.1","Type":"Full","FileName":"{linux_full}","SHA256":"{DIGEST_A}","Size":42}}]}}"#
        )
        .into_bytes();
        let windows = format!(
            r#"{{"Assets":[{{"Version":"0.10.14","Type":"Full","FileName":"{windows_full}"}}]}}"#
        )
        .into_bytes();
        let statuses = [linux_deb_url, sums_url, linux_full, windows_full]
            .into_iter()
            .map(|url| (url.to_owned(), 200))
            .collect();
        (linux_deb, linux_velopack, windows, statuses)
    }

    #[test]
    fn feed_audit_proves_both_platform_channels_and_predecessor_selection() {
        let (linux_deb, linux_velopack, windows, statuses) = feed_fixture();
        let audit = audit_public_update_feeds(PublicFeedAuditInput {
            repository: "acme/player",
            checked_at_unix: 44,
            expected_linux_version: "0.11.0-beta.1",
            installed_linux_version: "0.1.0-linux-alpha.112",
            expected_windows_version: "0.10.14",
            installed_windows_version: "0.10.13",
            linux_deb_feed: &linux_deb,
            linux_velopack_feed: &linux_velopack,
            windows_feed: &windows,
            http_statuses: &statuses,
        })
        .expect("feed audit should pass");
        assert!(audit.linux.update_offered);
        assert!(audit.windows.update_offered);
        assert_eq!(audit.referenced_assets.len(), 4);
        assert_eq!(audit.linux.deb_feed_sha256, sha256_hex(&linux_deb));
    }

    #[test]
    fn feed_audit_rejects_cross_release_asset_urls_and_missing_downloads() {
        let (linux_deb, mut linux_velopack, windows, statuses) = feed_fixture();
        linux_velopack = String::from_utf8(linux_velopack)
            .unwrap()
            .replace("linux-v0.11.0-beta.1", "linux-v0.1.0-alpha.112")
            .into_bytes();
        let errors = audit_public_update_feeds(PublicFeedAuditInput {
            repository: "acme/player",
            checked_at_unix: 44,
            expected_linux_version: "0.11.0-beta.1",
            installed_linux_version: "0.1.0-linux-alpha.112",
            expected_windows_version: "0.10.14",
            installed_windows_version: "0.10.13",
            linux_deb_feed: &linux_deb,
            linux_velopack_feed: &linux_velopack,
            windows_feed: &windows,
            http_statuses: &statuses,
        })
        .unwrap_err();
        assert!(errors.iter().any(|error| error.contains("not attached")));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("was not availability-checked"))
        );
    }

    #[test]
    fn feed_comparison_requires_both_channels_to_remain_byte_identical() {
        let (linux_deb, linux_velopack, windows, statuses) = feed_fixture();
        let input = |checked_at_unix| PublicFeedAuditInput {
            repository: "acme/player",
            checked_at_unix,
            expected_linux_version: "0.11.0-beta.1",
            installed_linux_version: "0.1.0-linux-alpha.112",
            expected_windows_version: "0.10.14",
            installed_windows_version: "0.10.13",
            linux_deb_feed: &linux_deb,
            linux_velopack_feed: &linux_velopack,
            windows_feed: &windows,
            http_statuses: &statuses,
        };
        let before = audit_public_update_feeds(input(44)).unwrap();
        let mut after = audit_public_update_feeds(input(45)).unwrap();
        let comparison = compare_public_feed_audits(&before, &after).unwrap();
        assert!(comparison.unchanged);

        after.windows.feed_sha256 = DIGEST_B.to_owned();
        assert!(
            compare_public_feed_audits(&before, &after).unwrap_err()[0]
                .contains("Windows public feed")
        );
    }

    #[test]
    fn archived_asset_digest_is_not_confused_with_another_checksum() {
        let mut input = release(1, "linux-v0.1.0-alpha.1", SHA_A);
        input.assets[0].digest = Some(format!("sha256:{DIGEST_B}"));
        let archive = build_linux_release_archive(
            "acme/player",
            42,
            vec![input],
            vec![tag("linux-v0.1.0-alpha.1", SHA_A)],
        )
        .unwrap();
        assert_eq!(archive.release_objects[0].assets[0].sha256, DIGEST_B);
    }

    #[test]
    fn public_beta_notes_require_completed_provenance_and_every_user_section() {
        use crate::acceptance_evidence::PackageArtifact;

        let identity = PackageIdentity {
            version: "0.11.0-beta.1".to_owned(),
            commit_sha: SHA_A.to_owned(),
            artifacts: vec![
                PackageArtifact {
                    kind: ArtifactKind::Debian,
                    file_name: "ok-player_0.11.0-beta.1_amd64.deb".to_owned(),
                    sha256: DIGEST_A.to_owned(),
                },
                PackageArtifact {
                    kind: ArtifactKind::AppImage,
                    file_name: "OK-Player-0.11.0-beta.1-x86_64.AppImage".to_owned(),
                    sha256: DIGEST_B.to_owned(),
                },
            ],
        };
        let notes = format!(
            "# 0.11.0-beta.1\n\n## User-visible changes\nDone.\n\n## Supported distro / session / package matrix\nMatrix.\n\n## Install, update, rollback, and uninstall\nSteps.\n\n## Checksums, source SHA, and provenance\n{SHA_A}\nok-player_0.11.0-beta.1_amd64.deb {DIGEST_A}\nOK-Player-0.11.0-beta.1-x86_64.AppImage {DIGEST_B}\n\n## Known limitations\nKnown.\n\n## Acceptance summary\nAccepted.\n"
        );
        assert_eq!(
            validate_public_beta_release_notes(&notes, &identity),
            Ok(())
        );

        let incomplete = notes.replace(DIGEST_B, "⟨appimage hash⟩");
        let errors = validate_public_beta_release_notes(&incomplete, &identity).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("template-only")));
        assert!(errors.iter().any(|error| error.contains("SHA-256")));
    }
}
