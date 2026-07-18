use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::acceptance_evidence::{CandidateUpgradeEvidence, PackageIdentity};
use okp_core::release_preparation::{
    GitHubReleaseInput, LinuxReleaseArchive, PublicFeedAuditInput, ReleaseRetainAllowlist,
    ResolvedGitTag, audit_public_update_feeds, build_linux_release_archive,
    compare_public_feed_audits, migration_anchor_check, plan_linux_release_cleanup,
    public_feed_asset_urls, validate_public_beta_release_notes,
};
use serde::Deserialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("archive-export") => archive_export(&args[1..]),
        Some("anchor-check") => anchor_check(&args[1..]),
        Some("cleanup-plan") => cleanup_plan(&args[1..]),
        Some("feed-audit") => feed_audit(&args[1..]),
        Some("feed-compare") => feed_compare(&args[1..]),
        Some("notes-validate") => notes_validate(&args[1..]),
        _ => Err(usage()),
    }
}

fn archive_export(args: &[String]) -> Result<(), String> {
    let repository = value(args, "--repository")?;
    let output = value(args, "--output")?;
    let releases: Vec<GitHubReleaseInput> =
        gh_paginated(&format!("repos/{repository}/releases?per_page=100"))?;
    let refs: Vec<GitRefInput> = gh_paginated(&format!(
        "repos/{repository}/git/matching-refs/tags/linux-v"
    ))?;
    let tags = refs
        .into_iter()
        .map(resolve_tag)
        .collect::<Result<Vec<_>, _>>()?;
    let archive = build_linux_release_archive(repository, unix_now()?, releases, tags)
        .map_err(|errors| errors.join("\n"))?;
    write_json(output, &archive)?;
    println!(
        "Archived {} Linux release objects, {} assets, and {} preserved git tags to {}.",
        archive.summary.release_object_count,
        archive.summary.release_asset_count,
        archive.summary.git_tag_count,
        output
    );
    Ok(())
}

fn anchor_check(args: &[String]) -> Result<(), String> {
    let archive_path = value(args, "--archive")?;
    let tag_name = value(args, "--tag")?;
    let output = value(args, "--output")?;
    let archive: LinuxReleaseArchive = read_json(archive_path)?;
    let release = archive
        .release_objects
        .iter()
        .find(|release| release.tag_name == tag_name)
        .ok_or_else(|| format!("migration anchor {tag_name} is missing from {archive_path}"))?;
    let statuses = release
        .assets
        .iter()
        .map(|asset| {
            Ok((
                asset.browser_download_url.clone(),
                http_head_status(&asset.browser_download_url)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let check = migration_anchor_check(&archive, tag_name, unix_now()?, &statuses)
        .map_err(|errors| errors.join("\n"))?;
    write_json(output, &check)?;
    println!(
        "Migration anchor {} remains downloadable with {} verified assets.",
        check.tag_name,
        check.assets.len()
    );
    Ok(())
}

fn cleanup_plan(args: &[String]) -> Result<(), String> {
    let archive_path = value(args, "--archive")?;
    let allowlist_path = value(args, "--allowlist")?;
    let batch_size = value(args, "--batch-size")?
        .parse::<usize>()
        .map_err(|error| format!("invalid --batch-size: {error}"))?;
    let output = value(args, "--output")?;
    let archive: LinuxReleaseArchive = read_json(archive_path)?;
    let allowlist: ReleaseRetainAllowlist = read_json(allowlist_path)?;
    let migration_evidence_validated = match optional_value(args, "--candidate-upgrade-evidence") {
        Some(path) => {
            let evidence: CandidateUpgradeEvidence = read_json(path)?;
            evidence
                .validate_cleanup_ready()
                .map_err(|errors| errors.join("\n"))?;
            true
        }
        None => false,
    };
    let plan = plan_linux_release_cleanup(
        &archive,
        &allowlist,
        batch_size,
        migration_evidence_validated,
    )
    .map_err(|errors| errors.join("\n"))?;
    write_json(output, &plan)?;
    println!(
        "Dry-run plan lists {} release-object deletions in {} bounded batches, preserves {} git tags, and has execution_ready={}.",
        plan.summary.planned_release_object_deletions,
        plan.summary.batch_count,
        plan.summary.preserved_git_tags,
        plan.execution_ready
    );
    Ok(())
}

fn feed_audit(args: &[String]) -> Result<(), String> {
    let repository = value(args, "--repository")?;
    let feed_base = value(args, "--feed-base")?.trim_end_matches('/');
    let expected_linux_version = value(args, "--expected-linux")?;
    let installed_linux_version = value(args, "--installed-linux")?;
    let expected_windows_version = value(args, "--expected-windows")?;
    let installed_windows_version = value(args, "--installed-windows")?;
    let output = value(args, "--output")?;
    let linux_deb_feed = http_get(&format!("{feed_base}/updates/linux/deb.linux.json"))?;
    let linux_velopack_feed = http_get(&format!("{feed_base}/updates/linux/releases.linux.json"))?;
    let windows_feed = http_get(&format!("{feed_base}/updates/win/releases.win.json"))?;
    let statuses = public_feed_asset_urls(&linux_deb_feed, &linux_velopack_feed, &windows_feed)
        .map_err(|errors| errors.join("\n"))?
        .into_iter()
        .map(|url| Ok((url.clone(), http_head_status(&url)?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let audit = audit_public_update_feeds(PublicFeedAuditInput {
        repository,
        checked_at_unix: unix_now()?,
        expected_linux_version,
        installed_linux_version,
        expected_windows_version,
        installed_windows_version,
        linux_deb_feed: &linux_deb_feed,
        linux_velopack_feed: &linux_velopack_feed,
        windows_feed: &windows_feed,
        http_statuses: &statuses,
    })
    .map_err(|errors| errors.join("\n"))?;
    write_json(output, &audit)?;
    println!(
        "Public feeds validated: Linux {} (update offered: {}), Windows {} (update offered: {}).",
        audit.linux.expected_version,
        audit.linux.update_offered,
        audit.windows.expected_version,
        audit.windows.update_offered
    );
    Ok(())
}

fn feed_compare(args: &[String]) -> Result<(), String> {
    let before_path = value(args, "--before")?;
    let after_path = value(args, "--after")?;
    let output = value(args, "--output")?;
    let before = read_json(before_path)?;
    let after = read_json(after_path)?;
    let comparison =
        compare_public_feed_audits(&before, &after).map_err(|errors| errors.join("\n"))?;
    write_json(output, &comparison)?;
    println!("Public Linux and Windows feeds remained unchanged across the cleanup batch.");
    Ok(())
}

fn notes_validate(args: &[String]) -> Result<(), String> {
    let notes_path = value(args, "--notes")?;
    let identity_path = value(args, "--identity")?;
    let notes = fs::read_to_string(notes_path).map_err(|error| format!("{notes_path}: {error}"))?;
    let identity: PackageIdentity = read_json(identity_path)?;
    validate_public_beta_release_notes(&notes, &identity).map_err(|errors| errors.join("\n"))?;
    println!(
        "Release notes contain the exact {} source and package provenance.",
        identity.version
    );
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
struct GitRefInput {
    #[serde(rename = "ref")]
    ref_name: String,
    object: GitObjectInput,
}

#[derive(Clone, Debug, Deserialize)]
struct GitObjectInput {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    url: String,
}

#[derive(Clone, Debug, Deserialize)]
struct AnnotatedTagInput {
    object: GitObjectInput,
}

fn resolve_tag(reference: GitRefInput) -> Result<ResolvedGitTag, String> {
    let tag_name = reference
        .ref_name
        .strip_prefix("refs/tags/")
        .ok_or_else(|| format!("unexpected git ref name: {}", reference.ref_name))?
        .to_owned();
    let ref_sha = reference.object.sha.clone();
    let mut object = reference.object;
    for _ in 0..8 {
        match object.kind.as_str() {
            "commit" => {
                return Ok(ResolvedGitTag {
                    tag_name,
                    ref_sha,
                    source_sha: object.sha,
                });
            }
            "tag" => {
                let annotated: AnnotatedTagInput = gh_json(api_endpoint(&object.url))?;
                object = annotated.object;
            }
            other => {
                return Err(format!(
                    "git tag {tag_name} resolves to unsupported object type {other}"
                ));
            }
        }
    }
    Err(format!(
        "git tag {tag_name} exceeded the annotated-tag resolution limit"
    ))
}

fn gh_paginated<T: serde::de::DeserializeOwned>(endpoint: &str) -> Result<Vec<T>, String> {
    let output = command_output(
        Command::new("gh").args(["api", "--paginate", "--slurp", endpoint]),
        "gh api",
    )?;
    let pages: Vec<Vec<T>> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("gh api {endpoint} returned invalid paginated JSON: {error}"))?;
    Ok(pages.into_iter().flatten().collect())
}

fn gh_json<T: serde::de::DeserializeOwned>(endpoint: &str) -> Result<T, String> {
    let output = command_output(Command::new("gh").args(["api", endpoint]), "gh api")?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("gh api {endpoint} returned invalid JSON: {error}"))
}

fn api_endpoint(url: &str) -> &str {
    url.strip_prefix("https://api.github.com/").unwrap_or(url)
}

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    Ok(command_output(
        Command::new("curl").args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "30",
            url,
        ]),
        "curl GET",
    )?
    .stdout)
}

fn http_head_status(url: &str) -> Result<u16, String> {
    let output = command_output(
        Command::new("curl").args([
            "--silent",
            "--show-error",
            "--location",
            "--head",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "30",
            url,
        ]),
        "curl HEAD",
    )?;
    String::from_utf8(output.stdout)
        .map_err(|error| format!("curl HEAD returned non-UTF-8 status: {error}"))?
        .trim()
        .parse::<u16>()
        .map_err(|error| format!("curl HEAD returned an invalid status for {url}: {error}"))
}

fn command_output(command: &mut Command, label: &str) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to run {label}: {error}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "{label} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{path}: {error}"))
}

fn write_json(path: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        return Err(format!(
            "output directory does not exist: {}",
            parent.display()
        ));
    }
    fs::write(path, bytes).map_err(|error| format!("{path}: {error}"))
}

fn unix_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))
}

fn value<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("missing {name}\n{}", usage()))
}

fn optional_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn usage() -> String {
    "usage:\n  okp-release-preparation archive-export --repository OWNER/REPO --output ARCHIVE.json\n  okp-release-preparation anchor-check --archive ARCHIVE.json --tag TAG --output CHECK.json\n  okp-release-preparation cleanup-plan --archive ARCHIVE.json --allowlist ALLOWLIST.json --batch-size N [--candidate-upgrade-evidence EVIDENCE.json] --output PLAN.json\n  okp-release-preparation feed-audit --repository OWNER/REPO --feed-base URL --expected-linux VERSION --installed-linux VERSION --expected-windows VERSION --installed-windows VERSION --output AUDIT.json\n  okp-release-preparation feed-compare --before AUDIT.json --after AUDIT.json --output COMPARISON.json\n  okp-release-preparation notes-validate --notes RELEASE-NOTES.md --identity PACKAGE-IDENTITY.json".to_owned()
}
