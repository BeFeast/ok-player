//! Repository-owned project outcome health contract (issue #412).
//!
//! The collector lives in `scripts/check-project-outcome.sh`; this module owns
//! every decision so network/process orchestration never becomes an untested
//! release state machine. Public Linux release age is deliberately diagnostic:
//! accepted rolling candidates are the development-delivery cadence signal.

use std::cmp::Ordering;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::candidate_channel::{AcceptanceStatus, CandidateFeed};
use crate::update_selection::compare_versions;
use crate::velopack_artifacts::LINUX_VELOPACK_PACKAGE_ID;

pub const DEFAULT_MAX_UNPUBLISHED_MAIN_LAG_SECONDS: u64 = 120 * 60;
pub const DEFAULT_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS: u64 = 120 * 60;
pub const DEFAULT_MAX_CANDIDATE_RUN_AGE_SECONDS: u64 = 90 * 60;
pub const DEFAULT_SOURCE_CI_GRACE_SECONDS: u64 = 15 * 60;
pub const DEFAULT_FUTURE_SKEW_SECONDS: u64 = 5 * 60;
pub const STABLE_RELEASE_FRESH_SECONDS: u64 = 48 * 60 * 60;
const CANDIDATE_NONDELIVERY_WINDOW_SECONDS: u64 = 2 * 60 * 60;
const WINDOWS_CANDIDATE_SCHEMA_VERSION: u64 = 1;
const WINDOWS_CANDIDATE_CHANNEL: &str = "win-candidate";
const WINDOWS_CANDIDATE_PACKAGE_ID: &str = "com.befeast.okplayer";
const WINDOWS_CANDIDATE_VERSION_BASE: &str = "0.11.0-beta.0";
const WINDOWS_CANDIDATE_FEED_NAME: &str = "releases.win-candidate.json";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FetchSnapshot {
    pub url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowSnapshot {
    pub name: String,
    pub head_sha: String,
    pub event: String,
    pub status: String,
    pub conclusion: String,
    #[serde(default)]
    pub created_at_utc: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduleRunSnapshot {
    pub head_sha: String,
    pub event: String,
    pub status: String,
    pub conclusion: String,
    pub completed_at_utc: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActiveCandidateRunSnapshot {
    pub head_sha: String,
    pub event: String,
    pub status: String,
    pub created_at_utc: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateWorkflowSnapshot {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub state_error: Option<String>,
    #[serde(default)]
    pub latest_completed_schedule: Option<ScheduleRunSnapshot>,
    #[serde(default)]
    pub schedule_error: Option<String>,
    #[serde(default)]
    pub latest_active_run: Option<ActiveCandidateRunSnapshot>,
    #[serde(default)]
    pub active_run_error: Option<String>,
    #[serde(default)]
    pub successful_delivery_runs: Vec<ScheduleRunSnapshot>,
    #[serde(default)]
    pub delivery_runs_error: Option<String>,
    #[serde(default)]
    pub consecutive_failed_runs: u64,
    #[serde(default)]
    pub last_failed_gate: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommitSnapshot {
    pub sha: String,
    pub committed_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateComparisonSnapshot {
    pub status: String,
    pub merge_base_sha: String,
    #[serde(default)]
    pub first_unpublished_commit: Option<CommitSnapshot>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateSourceSnapshot {
    #[serde(default)]
    pub candidate_sha: String,
    #[serde(default)]
    pub candidate_committed_at_utc: Option<String>,
    #[serde(default)]
    pub comparison: Option<CandidateComparisonSnapshot>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceSnapshot {
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub head_committed_at_utc: Option<String>,
    #[serde(default)]
    pub head_observed_at_utc: Option<String>,
    #[serde(default)]
    pub workflows: Vec<WorkflowSnapshot>,
    #[serde(default)]
    pub candidate_workflow: CandidateWorkflowSnapshot,
    #[serde(default)]
    pub candidate: CandidateSourceSnapshot,
    #[serde(default)]
    pub windows_candidate_workflow: CandidateWorkflowSnapshot,
    #[serde(default)]
    pub windows_candidate: CandidateSourceSnapshot,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectHealthSnapshot {
    pub checked_at_unix: u64,
    #[serde(default = "default_source_ci_grace")]
    pub source_ci_grace_seconds: u64,
    #[serde(default = "default_max_unpublished_main_lag")]
    pub max_unpublished_main_lag_seconds: u64,
    #[serde(default = "default_max_candidate_schedule_age")]
    pub max_candidate_schedule_age_seconds: u64,
    #[serde(default = "default_max_candidate_run_age")]
    pub max_candidate_run_age_seconds: u64,
    #[serde(default = "default_future_skew")]
    pub future_skew_seconds: u64,
    pub source: SourceSnapshot,
    pub windows_feed: FetchSnapshot,
    #[serde(default)]
    pub windows_candidate_manifest: FetchSnapshot,
    #[serde(default)]
    pub windows_candidate_feed: FetchSnapshot,
    pub candidate_feed: FetchSnapshot,
    pub stable_linux_releases: FetchSnapshot,
}

fn default_max_unpublished_main_lag() -> u64 {
    DEFAULT_MAX_UNPUBLISHED_MAIN_LAG_SECONDS
}

fn default_source_ci_grace() -> u64 {
    DEFAULT_SOURCE_CI_GRACE_SECONDS
}

fn default_max_candidate_schedule_age() -> u64 {
    DEFAULT_MAX_CANDIDATE_SCHEDULE_AGE_SECONDS
}

fn default_max_candidate_run_age() -> u64 {
    DEFAULT_MAX_CANDIDATE_RUN_AGE_SECONDS
}

fn default_future_skew() -> u64 {
    DEFAULT_FUTURE_SKEW_SECONDS
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthStatus {
    Pass,
    Fail,
    Warning,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthCheck {
    pub name: String,
    pub blocking: bool,
    pub status: HealthStatus,
    pub summary: String,
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectHealthOutcome {
    pub healthy: bool,
    pub checked_at_unix: u64,
    pub source_ci_grace_seconds: u64,
    pub max_unpublished_main_lag_seconds: u64,
    pub max_candidate_schedule_age_seconds: u64,
    pub max_candidate_run_age_seconds: u64,
    pub checks: Vec<HealthCheck>,
}

impl ProjectHealthSnapshot {
    pub fn evaluate(&self) -> ProjectHealthOutcome {
        let checks = vec![
            evaluate_source(
                &self.source,
                self.checked_at_unix,
                self.source_ci_grace_seconds,
                self.future_skew_seconds,
            ),
            evaluate_windows_feed(&self.windows_feed),
            evaluate_windows_candidate(
                &self.windows_candidate_manifest,
                &self.windows_candidate_feed,
                &self.source,
                self.checked_at_unix,
                self.max_unpublished_main_lag_seconds,
                self.future_skew_seconds,
            ),
            evaluate_candidate(
                &self.candidate_feed,
                &self.source,
                self.checked_at_unix,
                self.max_unpublished_main_lag_seconds,
                self.max_candidate_schedule_age_seconds,
                self.max_candidate_run_age_seconds,
                self.future_skew_seconds,
            ),
            evaluate_stable_release(&self.stable_linux_releases, self.checked_at_unix),
        ];
        let healthy = checks.iter().all(|check| {
            !check.blocking || !matches!(check.status, HealthStatus::Fail | HealthStatus::Unknown)
        });
        ProjectHealthOutcome {
            healthy,
            checked_at_unix: self.checked_at_unix,
            source_ci_grace_seconds: self.source_ci_grace_seconds,
            max_unpublished_main_lag_seconds: self.max_unpublished_main_lag_seconds,
            max_candidate_schedule_age_seconds: self.max_candidate_schedule_age_seconds,
            max_candidate_run_age_seconds: self.max_candidate_run_age_seconds,
            checks,
        }
    }
}

fn evaluate_source(
    source: &SourceSnapshot,
    now: u64,
    grace_seconds: u64,
    future_skew_seconds: u64,
) -> HealthCheck {
    let mut failures = Vec::new();
    let mut pending = Vec::new();
    if let Some(error) = nonempty(source.error.as_deref()) {
        failures.push(format!("source/main query failed: {error}"));
    }
    if !is_hex(&source.head_sha, 40) {
        failures.push("source/main did not resolve to an exact 40-character SHA".to_owned());
    }
    for required in ["CI", "Rust"] {
        match source.workflows.iter().find(|run| run.name == required) {
            None => pending.push(format!("source/main has no {required} workflow result yet")),
            Some(run) => {
                if run.head_sha != source.head_sha {
                    pending.push(format!(
                        "source/main {required} result is still for {}, expected {}",
                        run.head_sha, source.head_sha
                    ));
                    continue;
                }
                if run.event != "push" {
                    failures.push(format!(
                        "source/main {required} result has event {}, expected push",
                        run.event
                    ));
                }
                match run.status.as_str() {
                    "completed" if run.conclusion == "success" => {}
                    "completed" => failures.push(format!(
                        "source/main {required} is completed/{}",
                        run.conclusion
                    )),
                    "queued" | "in_progress" | "pending" | "requested" | "waiting"
                        if run.conclusion.is_empty() =>
                    {
                        pending.push(format!(
                            "source/main {required} is {}/{}",
                            run.status, run.conclusion
                        ));
                    }
                    _ => failures.push(format!(
                        "source/main {required} has unexpected status {}/{}",
                        run.status, run.conclusion
                    )),
                }
            }
        }
    }
    if !failures.is_empty() {
        failures.extend(pending);
        return check(
            "source-main-ci",
            true,
            failures,
            format!("CI and Rust succeeded for source/main {}", source.head_sha),
        );
    }
    if pending.is_empty() {
        return check(
            "source-main-ci",
            true,
            Vec::new(),
            format!("CI and Rust succeeded for source/main {}", source.head_sha),
        );
    }

    let (settling_anchor, anchor_name) = match source.head_observed_at_utc.as_deref() {
        Some(value) => (Some(value), "observation"),
        None => (source.head_committed_at_utc.as_deref(), "commit"),
    };
    let settling_started_at = settling_anchor.and_then(parse_utc_timestamp);
    match settling_started_at {
        Some(timestamp) if timestamp <= now.saturating_add(future_skew_seconds) => {
            let age = now.saturating_sub(timestamp);
            if age <= grace_seconds {
                return HealthCheck {
                    name: "source-main-ci".to_owned(),
                    blocking: true,
                    status: HealthStatus::Warning,
                    summary: format!(
                        "source/main CI is settling for {} ({age}s into {grace_seconds}s grace)",
                        source.head_sha
                    ),
                    details: pending,
                    reason_codes: vec!["source-main-ci-settling".to_owned()],
                };
            }
            pending.push(format!(
                "source/main CI is still pending {age}s after the {anchor_name}, exceeding {grace_seconds}s grace"
            ));
        }
        Some(timestamp) => pending.push(format!(
            "source/main {anchor_name} timestamp is {} seconds in the future",
            timestamp - now
        )),
        None => pending.push(format!(
            "source/main {anchor_name} timestamp is unavailable or not valid UTC RFC 3339"
        )),
    }
    check(
        "source-main-ci",
        true,
        pending,
        format!("CI and Rust succeeded for source/main {}", source.head_sha),
    )
}

#[derive(Deserialize)]
struct WindowsFeed {
    #[serde(rename = "Assets")]
    assets: Vec<WindowsAsset>,
}

#[derive(Deserialize)]
struct WindowsAsset {
    #[serde(rename = "PackageId")]
    package_id: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FileName")]
    url: String,
    #[serde(rename = "SHA256")]
    sha256: String,
    #[serde(rename = "Size")]
    size: u64,
}

fn evaluate_windows_feed(fetch: &FetchSnapshot) -> HealthCheck {
    let mut details = fetch_failure(fetch, "Windows static feed");
    let mut summary = format!("Windows static feed is healthy at {}", fetch.url);
    if details.is_empty() {
        match serde_json::from_str::<WindowsFeed>(fetch.body.as_deref().unwrap_or_default()) {
            Err(error) => details.push(format!("Windows static feed is malformed: {error}")),
            Ok(feed) => {
                if feed.assets.is_empty() {
                    details.push("Windows static feed contains no assets".to_owned());
                }
                if !feed.assets.iter().any(|asset| asset.kind == "Full") {
                    details.push("Windows static feed contains no Full package".to_owned());
                }
                for asset in &feed.assets {
                    if asset.package_id.trim().is_empty() || asset.version.trim().is_empty() {
                        details.push(
                            "Windows static feed has an incomplete package identity".to_owned(),
                        );
                    }
                    if !is_https_url(&asset.url) {
                        details.push(format!(
                            "Windows package URL is not absolute HTTPS: {}",
                            asset.url
                        ));
                    }
                    if !is_hex(&asset.sha256, 64) || asset.size == 0 {
                        details.push(format!(
                            "Windows package {} has an invalid SHA-256 or size",
                            asset.url
                        ));
                    }
                }
                summary = format!(
                    "Windows static feed exposes {} verified asset(s)",
                    feed.assets.len()
                );
            }
        }
    }
    check("windows-static-feed", true, details, summary)
}

#[derive(Deserialize)]
struct WindowsCandidateManifest {
    schema_version: u64,
    channel: String,
    source_sha: String,
    build_number: u64,
    version: String,
    builder: String,
    timestamp_utc: String,
    feed: WindowsCandidateArtifact,
    artifacts: Vec<WindowsCandidateArtifact>,
}

#[derive(Deserialize)]
struct WindowsCandidateArtifact {
    name: String,
    sha256: String,
    size: u64,
    version: Option<String>,
    current: bool,
}

#[derive(Deserialize)]
struct WindowsCandidateFeed {
    #[serde(rename = "Assets")]
    assets: Vec<WindowsCandidateFeedAsset>,
}

#[derive(Deserialize)]
struct WindowsCandidateFeedAsset {
    #[serde(rename = "PackageId")]
    package_id: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FileName")]
    file_name: String,
    #[serde(rename = "SHA256")]
    sha256: String,
    #[serde(rename = "Size")]
    size: u64,
}

fn evaluate_windows_candidate(
    manifest_fetch: &FetchSnapshot,
    feed_fetch: &FetchSnapshot,
    source: &SourceSnapshot,
    now: u64,
    max_lag: u64,
    future_skew: u64,
) -> HealthCheck {
    let workflow = &source.windows_candidate_workflow;
    if workflow.state == "active"
        && workflow.state_error.is_none()
        && workflow.latest_completed_schedule.is_none()
        && workflow.schedule_error.is_none()
        && manifest_fetch.body.is_none()
        && feed_fetch.body.is_none()
    {
        return HealthCheck {
            name: "windows-candidate-delivery".to_owned(),
            blocking: true,
            status: HealthStatus::Warning,
            summary:
                "Windows candidate lane is bootstrapping and has no completed automatic run history"
                    .to_owned(),
            details: Vec::new(),
            reason_codes: vec!["windows-candidate-bootstrap".to_owned()],
        };
    }

    let mut details = Vec::new();
    let mut reason_codes = Vec::new();
    validate_windows_candidate_failure_streak(workflow, &mut details, &mut reason_codes);
    validate_windows_candidate_workflow_state(workflow, &mut details, &mut reason_codes);
    details.extend(fetch_failure(
        manifest_fetch,
        "Windows candidate identity manifest",
    ));
    details.extend(fetch_failure(feed_fetch, "Windows candidate feed"));
    let mut summary = format!("Windows candidate feed is healthy at {}", feed_fetch.url);
    if !details.is_empty() {
        return check_with_reason_codes(
            "windows-candidate-delivery",
            true,
            details,
            summary,
            reason_codes,
        );
    }

    let manifest = match serde_json::from_str::<WindowsCandidateManifest>(
        manifest_fetch.body.as_deref().unwrap_or_default(),
    ) {
        Ok(manifest) => manifest,
        Err(error) => {
            details.push(format!(
                "Windows candidate identity manifest is malformed: {error}"
            ));
            return check_with_reason_codes(
                "windows-candidate-delivery",
                true,
                details,
                summary,
                reason_codes,
            );
        }
    };
    let feed = match serde_json::from_str::<WindowsCandidateFeed>(
        feed_fetch.body.as_deref().unwrap_or_default(),
    ) {
        Ok(feed) => feed,
        Err(error) => {
            details.push(format!("Windows candidate feed is malformed: {error}"));
            return check_with_reason_codes(
                "windows-candidate-delivery",
                true,
                details,
                summary,
                reason_codes,
            );
        }
    };

    validate_windows_candidate_identity(&manifest, &feed, manifest_fetch, feed_fetch, &mut details);
    if max_lag == 0 {
        details.push("Windows candidate unpublished-main lag limit must be positive".to_owned());
    }
    let candidate_timestamp = validate_windows_candidate_timestamp(
        "Windows candidate timestamp",
        &manifest.timestamp_utc,
        now,
        future_skew,
        &mut details,
    );
    let source_timestamp = validate_windows_candidate_source(
        &manifest,
        source,
        now,
        max_lag,
        future_skew,
        &mut details,
        &mut summary,
    );
    if let (Some(candidate_timestamp), Some(source_timestamp)) =
        (candidate_timestamp, source_timestamp)
        && candidate_timestamp < source_timestamp
    {
        details.push(format!(
            "Windows candidate timestamp {} predates source commit timestamp {}",
            manifest.timestamp_utc,
            source
                .windows_candidate
                .candidate_committed_at_utc
                .as_deref()
                .unwrap_or_default()
        ));
    }

    check_with_reason_codes(
        "windows-candidate-delivery",
        true,
        details,
        summary,
        reason_codes,
    )
}

fn validate_windows_candidate_failure_streak(
    workflow: &CandidateWorkflowSnapshot,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) {
    if workflow.consecutive_failed_runs < 2 {
        return;
    }
    let gate = nonempty(workflow.last_failed_gate.as_deref()).unwrap_or("unavailable");
    details.push(format!(
        "Windows candidate builder failing at gate {gate} ({} consecutive)",
        workflow.consecutive_failed_runs
    ));
    reason_codes.push("windows-candidate-builds-failing".to_owned());
}

fn validate_windows_candidate_workflow_state(
    workflow: &CandidateWorkflowSnapshot,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) {
    if let Some(error) = nonempty(workflow.state_error.as_deref()) {
        details.push(format!(
            "Windows Candidate workflow state query failed: {error}"
        ));
        reason_codes.push("windows-candidate-workflow-state-unavailable".to_owned());
        return;
    }
    if workflow.state.trim().is_empty() {
        details.push("Windows Candidate workflow state is unavailable".to_owned());
        reason_codes.push("windows-candidate-workflow-state-unavailable".to_owned());
    } else if workflow.state != "active" {
        details.push(format!(
            "Windows Candidate workflow state is {}, expected active",
            workflow.state
        ));
        reason_codes.push("windows-candidate-workflow-inactive".to_owned());
    }
}

fn validate_windows_candidate_identity(
    manifest: &WindowsCandidateManifest,
    feed: &WindowsCandidateFeed,
    manifest_fetch: &FetchSnapshot,
    feed_fetch: &FetchSnapshot,
    details: &mut Vec<String>,
) {
    if manifest.schema_version != WINDOWS_CANDIDATE_SCHEMA_VERSION {
        details.push(format!(
            "Windows candidate schema is {}, expected {}",
            manifest.schema_version, WINDOWS_CANDIDATE_SCHEMA_VERSION
        ));
    }
    if manifest.channel != WINDOWS_CANDIDATE_CHANNEL {
        details.push(format!(
            "Windows candidate channel is {}, expected {WINDOWS_CANDIDATE_CHANNEL}",
            manifest.channel
        ));
    }
    if !is_hex(&manifest.source_sha, 40) {
        details.push("Windows candidate source SHA is invalid".to_owned());
    }
    let expected_version = format!("{WINDOWS_CANDIDATE_VERSION_BASE}.{}", manifest.build_number);
    if manifest.build_number == 0 || manifest.version != expected_version {
        details.push(format!(
            "Windows candidate version {} does not encode monotonic build {}",
            manifest.version, manifest.build_number
        ));
    }
    if manifest.builder.trim().is_empty() {
        details.push("Windows candidate builder identity is empty".to_owned());
    }
    if !manifest_fetch.url.ends_with("candidate.windows.json") || !is_https_url(&manifest_fetch.url)
    {
        details.push(
            "Windows candidate manifest URL must be absolute HTTPS and end in candidate.windows.json"
                .to_owned(),
        );
    }
    if !feed_fetch.url.ends_with(WINDOWS_CANDIDATE_FEED_NAME) || !is_https_url(&feed_fetch.url) {
        details.push(format!(
            "Windows candidate feed URL must be absolute HTTPS and end in {WINDOWS_CANDIDATE_FEED_NAME}"
        ));
    }

    let feed_body = feed_fetch.body.as_deref().unwrap_or_default();
    let feed_sha = sha256_hex(feed_body.as_bytes());
    if manifest.feed.name != WINDOWS_CANDIDATE_FEED_NAME
        || manifest.feed.version.as_deref() != Some(manifest.version.as_str())
        || !manifest.feed.current
        || manifest.feed.size != feed_body.len() as u64
        || !manifest.feed.sha256.eq_ignore_ascii_case(&feed_sha)
    {
        details.push("Windows candidate feed does not match its identity manifest".to_owned());
    }

    let expected_full_name = format!(
        "{WINDOWS_CANDIDATE_PACKAGE_ID}-{}-{WINDOWS_CANDIDATE_CHANNEL}-full.nupkg",
        manifest.version
    );
    let current_full: Vec<_> = feed
        .assets
        .iter()
        .filter(|asset| {
            asset.package_id == WINDOWS_CANDIDATE_PACKAGE_ID
                && asset.version == manifest.version
                && asset.kind.eq_ignore_ascii_case("Full")
        })
        .collect();
    if current_full.len() != 1 {
        details.push(
            "Windows candidate feed must contain exactly one current manifest-bound Full package"
                .to_owned(),
        );
        return;
    }
    let full = current_full[0];
    if full.file_name != expected_full_name || !is_hex(&full.sha256, 64) || full.size == 0 {
        details.push("Windows candidate Full package identity is invalid".to_owned());
    }
    let artifact_matches = manifest.artifacts.iter().filter(|artifact| {
        artifact.name == full.file_name
            && artifact.version.as_deref() == Some(full.version.as_str())
            && artifact.current
            && artifact.size == full.size
            && artifact.sha256.eq_ignore_ascii_case(&full.sha256)
    });
    if artifact_matches.count() != 1 {
        details.push(
            "Windows candidate Full package does not match its manifest artifact identity"
                .to_owned(),
        );
    }
}

fn validate_windows_candidate_source(
    manifest: &WindowsCandidateManifest,
    source: &SourceSnapshot,
    now: u64,
    max_lag: u64,
    future_skew: u64,
    details: &mut Vec<String>,
    summary: &mut String,
) -> Option<u64> {
    let evidence = &source.windows_candidate;
    if let Some(error) = nonempty(evidence.error.as_deref()) {
        details.push(format!("Windows candidate source query failed: {error}"));
    }
    if !same_sha(&evidence.candidate_sha, &manifest.source_sha) {
        details.push(format!(
            "Windows candidate source evidence is for {}, expected {}",
            evidence.candidate_sha, manifest.source_sha
        ));
    }
    let candidate_committed_at = match evidence.candidate_committed_at_utc.as_deref() {
        Some(value) => validate_timestamp(
            "Windows candidate source commit timestamp",
            value,
            now,
            future_skew,
            details,
        ),
        None => {
            details.push("Windows candidate source commit timestamp is unavailable".to_owned());
            None
        }
    };

    match classify_candidate_source(evidence, &source.head_sha) {
        CandidateSourceRelation::Equal => {
            let age = parse_windows_candidate_timestamp(&manifest.timestamp_utc)
                .map(|timestamp| now.saturating_sub(timestamp))
                .unwrap_or_default();
            *summary = format!(
                "Windows candidate {} build {} exactly matches current main; feed timestamp age {age}s is non-blocking",
                manifest.version, manifest.build_number
            );
        }
        CandidateSourceRelation::Ancestor => match evidence
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.first_unpublished_commit.as_ref())
        {
            None => details.push(
                "Windows candidate is behind main but the first unpublished commit is unavailable"
                    .to_owned(),
            ),
            Some(commit) => {
                if !is_hex(&commit.sha, 40) || same_sha(&commit.sha, &manifest.source_sha) {
                    details.push(
                        "Windows first unpublished main commit has an invalid identity".to_owned(),
                    );
                }
                if let Some(unpublished_at) = validate_timestamp(
                    "Windows first unpublished main commit timestamp",
                    &commit.committed_at_utc,
                    now,
                    future_skew,
                    details,
                ) {
                    if candidate_committed_at.is_some_and(|source_at| unpublished_at < source_at) {
                        details.push(
                            "Windows first unpublished main commit predates the candidate source"
                                .to_owned(),
                        );
                    }
                    let lag = now.saturating_sub(unpublished_at);
                    if lag > max_lag {
                        details.push(format!(
                            "Windows candidate is stale: unpublished main lag {lag}s exceeds {max_lag}s"
                        ));
                    }
                    *summary = format!(
                        "Windows candidate {} build {} is behind current main by {lag}s from first unpublished commit {}",
                        manifest.version, manifest.build_number, commit.sha
                    );
                }
            }
        },
        CandidateSourceRelation::NotAncestor => details.push(format!(
            "Windows candidate source {} is not an ancestor of current main {}",
            manifest.source_sha, source.head_sha
        )),
        CandidateSourceRelation::Unknown => {
            details.push("Windows candidate source relation to current main is unknown".to_owned())
        }
    }

    candidate_committed_at
}

fn validate_windows_candidate_timestamp(
    label: &str,
    value: &str,
    now: u64,
    future_skew: u64,
    details: &mut Vec<String>,
) -> Option<u64> {
    match parse_windows_candidate_timestamp(value) {
        None => {
            details.push(format!("{label} is not valid UTC RFC 3339: {value}"));
            None
        }
        Some(timestamp) if timestamp > now.saturating_add(future_skew) => {
            details.push(format!(
                "{label} is {} seconds in the future",
                timestamp - now
            ));
            Some(timestamp)
        }
        Some(timestamp) => Some(timestamp),
    }
}

fn parse_windows_candidate_timestamp(value: &str) -> Option<u64> {
    let utc = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))?;
    if utc.len() < 19 || utc.as_bytes().get(19).is_some_and(|byte| *byte != b'.') {
        return None;
    }
    let normalized = format!("{}Z", &utc[..19]);
    parse_utc_timestamp(&normalized)
}

fn evaluate_candidate(
    fetch: &FetchSnapshot,
    source: &SourceSnapshot,
    now: u64,
    max_lag: u64,
    max_schedule_age: u64,
    max_active_run_age: u64,
    future_skew: u64,
) -> HealthCheck {
    let mut details = Vec::new();
    let mut reason_codes = Vec::new();
    let mut summary = format!("Linux candidate feed is healthy at {}", fetch.url);
    validate_candidate_workflow_state(source, &mut details, &mut reason_codes);
    validate_candidate_failure_streak(source, &mut details, &mut reason_codes);
    details.extend(fetch_failure(fetch, "Linux candidate feed"));
    if !details.is_empty() {
        return check_with_reason_codes(
            "linux-candidate-delivery",
            true,
            details,
            summary,
            reason_codes,
        );
    }

    let body = fetch.body.as_deref().unwrap_or_default();
    let value = match serde_json::from_str::<Value>(body) {
        Ok(value) => value,
        Err(error) => {
            details.push(format!("Linux candidate feed is malformed JSON: {error}"));
            return check_with_reason_codes(
                "linux-candidate-delivery",
                true,
                details,
                summary,
                reason_codes,
            );
        }
    };
    let missing = missing_candidate_identity(&value);
    if !missing.is_empty() {
        details.push(format!(
            "Linux candidate package identity is incomplete: missing {}",
            missing.join(", ")
        ));
        return check_with_reason_codes(
            "linux-candidate-delivery",
            true,
            details,
            summary,
            reason_codes,
        );
    }
    let feed = match serde_json::from_value::<CandidateFeed>(value) {
        Ok(feed) => feed,
        Err(error) => {
            details.push(format!("Linux candidate feed schema is invalid: {error}"));
            return check_with_reason_codes(
                "linux-candidate-delivery",
                true,
                details,
                summary,
                reason_codes,
            );
        }
    };

    if !feed.is_candidate_channel() {
        details.push(format!(
            "Linux candidate feed declares channel {:?}, expected candidate",
            feed.channel
        ));
    }
    if feed.acceptance != AcceptanceStatus::Accepted {
        details.push(format!(
            "Linux candidate {} is {:?}, expected accepted",
            feed.version, feed.acceptance
        ));
    }
    if !feed.has_valid_identity() {
        details
            .push("Linux candidate has an invalid source or package SHA-256 identity".to_owned());
    }
    validate_candidate_package(&feed, &fetch.url, &mut details);
    validate_candidate_monotonicity(&feed, &mut details);
    if max_lag == 0 {
        details.push("Linux candidate unpublished-main lag limit must be positive".to_owned());
    }
    if max_schedule_age == 0 {
        details.push("Linux candidate schedule freshness limit must be positive".to_owned());
    }
    if max_active_run_age == 0 {
        details.push("Linux candidate active-run age limit must be positive".to_owned());
    }

    let source_relation = classify_candidate_source(&source.candidate, &source.head_sha);
    let active_run_summary = validate_candidate_schedule(
        source,
        source_relation,
        CandidateScheduleTiming {
            now,
            max_schedule_age,
            max_active_run_age,
            future_skew,
        },
        &mut details,
        &mut reason_codes,
    );
    let nondelivery_summary = validate_successful_candidate_nondelivery(
        source,
        source_relation,
        active_run_summary.is_some(),
        now,
        CANDIDATE_NONDELIVERY_WINDOW_SECONDS,
        future_skew,
        &mut details,
        &mut reason_codes,
    );

    let candidate_timestamp = validate_timestamp(
        "Linux candidate timestamp",
        &feed.timestamp_utc,
        now,
        future_skew,
        &mut details,
    );
    let source_timestamp = validate_candidate_source(
        &feed,
        source,
        now,
        max_lag,
        future_skew,
        &mut details,
        &mut summary,
    );
    if let (Some(candidate_timestamp), Some(source_timestamp)) =
        (candidate_timestamp, source_timestamp)
        && candidate_timestamp < source_timestamp
    {
        details.push(format!(
            "Linux candidate timestamp {} predates source commit timestamp {}",
            feed.timestamp_utc,
            source
                .candidate
                .candidate_committed_at_utc
                .as_deref()
                .unwrap_or_default()
        ));
    }

    let mut check = check_with_reason_codes(
        "linux-candidate-delivery",
        true,
        details,
        summary,
        reason_codes,
    );
    if check.status == HealthStatus::Pass
        && let Some(active_run_summary) = active_run_summary
    {
        check.status = HealthStatus::Warning;
        check.summary = active_run_summary;
    } else if check.status == HealthStatus::Pass
        && let Some(nondelivery_summary) = nondelivery_summary
    {
        check.status = HealthStatus::Warning;
        check.summary = nondelivery_summary;
    }
    check
}

#[allow(clippy::too_many_arguments)]
fn validate_successful_candidate_nondelivery(
    source: &SourceSnapshot,
    source_relation: CandidateSourceRelation,
    active_tip_run: bool,
    now: u64,
    attempt_window: u64,
    future_skew: u64,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) -> Option<String> {
    if source_relation != CandidateSourceRelation::Ancestor || active_tip_run {
        return None;
    }

    let workflow = &source.candidate_workflow;
    if let Some(error) = nonempty(workflow.delivery_runs_error.as_deref()) {
        details.push(format!(
            "Linux Candidate successful delivery run query failed while main has advanced: {error}"
        ));
        reason_codes.push("candidate-delivery-runs-unavailable".to_owned());
        return None;
    }

    let first_unpublished_at = source
        .candidate
        .comparison
        .as_ref()
        .and_then(|comparison| comparison.first_unpublished_commit.as_ref())
        .and_then(|commit| parse_utc_timestamp(&commit.committed_at_utc))?;
    let window_start = now.saturating_sub(attempt_window);
    let mut successful_attempts = 0_u64;
    let mut newest_url = None;
    for run in &workflow.successful_delivery_runs {
        if !matches!(run.event.as_str(), "schedule" | "workflow_dispatch")
            || run.status != "completed"
            || run.conclusion != "success"
        {
            details.push(format!(
                "Linux Candidate delivery attempt evidence is {}/{}/{}, expected schedule or workflow_dispatch/completed/success",
                run.event, run.status, run.conclusion
            ));
            reason_codes.push("candidate-delivery-runs-unavailable".to_owned());
            return None;
        }
        let previous_detail_count = details.len();
        let Some(completed_at) = validate_timestamp(
            "Linux Candidate successful delivery run timestamp",
            &run.completed_at_utc,
            now,
            future_skew,
            details,
        ) else {
            reason_codes.push("candidate-delivery-runs-unavailable".to_owned());
            return None;
        };
        if details.len() != previous_detail_count {
            reason_codes.push("candidate-delivery-runs-unavailable".to_owned());
            return None;
        }
        if completed_at >= window_start && completed_at >= first_unpublished_at {
            successful_attempts += 1;
            if newest_url.is_none() && !run.url.trim().is_empty() {
                newest_url = Some(run.url.as_str());
            }
        }
    }

    if successful_attempts == 0 {
        return None;
    }

    reason_codes.push("candidate-delivery-not-published".to_owned());
    let run_reference = newest_url.map_or_else(String::new, |url| format!("; newest run {url}"));
    if successful_attempts == 1 {
        return Some(format!(
            "Linux Candidate completed one successful run after main advanced, but the accepted feed still points to {} instead of current main {}; workflow success is non-delivery and recovery dispatch is required{run_reference}",
            source.candidate.candidate_sha, source.head_sha
        ));
    }

    details.push(format!(
        "Linux Candidate completed {successful_attempts} successful runs within {attempt_window}s after main advanced, but the accepted feed still points to {} instead of current main {}; workflow success is non-delivery{run_reference}",
        source.candidate.candidate_sha, source.head_sha
    ));
    None
}

fn validate_candidate_failure_streak(
    source: &SourceSnapshot,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) {
    let workflow = &source.candidate_workflow;
    if workflow.consecutive_failed_runs < 2 {
        return;
    }
    let gate = nonempty(workflow.last_failed_gate.as_deref()).unwrap_or("unavailable");
    details.push(format!(
        "candidate builds failing: gate {gate} ({} consecutive)",
        workflow.consecutive_failed_runs
    ));
    reason_codes.push("candidate-builds-failing".to_owned());
}

fn validate_candidate_workflow_state(
    source: &SourceSnapshot,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) {
    let workflow = &source.candidate_workflow;
    if let Some(error) = nonempty(workflow.state_error.as_deref()) {
        details.push(format!(
            "Linux Candidate workflow state query failed: {error}"
        ));
        reason_codes.push("candidate-workflow-state-unavailable".to_owned());
        return;
    }
    if workflow.state.trim().is_empty() {
        details.push("Linux Candidate workflow state is unavailable".to_owned());
        reason_codes.push("candidate-workflow-state-unavailable".to_owned());
    } else if workflow.state != "active" {
        details.push(format!(
            "Linux Candidate workflow state is {}, expected active",
            workflow.state
        ));
        reason_codes.push("candidate-workflow-inactive".to_owned());
    }
}

#[derive(Clone, Copy)]
struct CandidateScheduleTiming {
    now: u64,
    max_schedule_age: u64,
    max_active_run_age: u64,
    future_skew: u64,
}

fn validate_candidate_schedule(
    source: &SourceSnapshot,
    source_relation: CandidateSourceRelation,
    timing: CandidateScheduleTiming,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) -> Option<String> {
    let workflow = &source.candidate_workflow;
    if workflow.state != "active" || source_relation != CandidateSourceRelation::Ancestor {
        return None;
    }
    if let Some(error) = nonempty(workflow.schedule_error.as_deref()) {
        details.push(format!(
            "Linux Candidate completed schedule query failed while main has advanced: {error}"
        ));
        reason_codes.push("candidate-schedule-unavailable".to_owned());
        return None;
    }
    let active_run_summary =
        validate_active_candidate_run(workflow, &source.head_sha, timing, details, reason_codes);
    let Some(run) = workflow.latest_completed_schedule.as_ref() else {
        if let Some(summary) = active_run_summary {
            return Some(summary);
        }
        if let Some(error) = nonempty(workflow.active_run_error.as_deref()) {
            details.push(format!(
                "Linux Candidate active run query failed while no fresh completed schedule exists: {error}"
            ));
            reason_codes.push("candidate-active-run-unavailable".to_owned());
        }
        details.push(format!(
            "Linux Candidate workflow has no completed schedule run within {}s while main has advanced",
            timing.max_schedule_age
        ));
        reason_codes.push("candidate-schedule-stale".to_owned());
        return None;
    };
    if run.event != "schedule" || run.status != "completed" {
        details.push(format!(
            "Linux Candidate schedule evidence is {}/{}, expected schedule/completed",
            run.event, run.status
        ));
        reason_codes.push("candidate-schedule-unavailable".to_owned());
        return None;
    }
    let previous_detail_count = details.len();
    let Some(completed_at) = validate_timestamp(
        "Linux Candidate completed schedule timestamp",
        &run.completed_at_utc,
        timing.now,
        timing.future_skew,
        details,
    ) else {
        reason_codes.push("candidate-schedule-unavailable".to_owned());
        return None;
    };
    if details.len() != previous_detail_count {
        reason_codes.push("candidate-schedule-unavailable".to_owned());
        return None;
    }
    let age = timing.now.saturating_sub(completed_at);
    if age > timing.max_schedule_age {
        if let Some(summary) = active_run_summary {
            return Some(summary);
        }
        if let Some(error) = nonempty(workflow.active_run_error.as_deref()) {
            details.push(format!(
                "Linux Candidate active run query failed while the completed schedule is stale: {error}"
            ));
            reason_codes.push("candidate-active-run-unavailable".to_owned());
        }
        details.push(format!(
            "Linux Candidate workflow has no completed schedule run within {}s while main has advanced; latest completed schedule run is {age}s old",
            timing.max_schedule_age
        ));
        reason_codes.push("candidate-schedule-stale".to_owned());
    }
    active_run_summary
}

fn validate_active_candidate_run(
    workflow: &CandidateWorkflowSnapshot,
    main_sha: &str,
    timing: CandidateScheduleTiming,
    details: &mut Vec<String>,
    reason_codes: &mut Vec<String>,
) -> Option<String> {
    let run = workflow.latest_active_run.as_ref()?;
    if !same_sha(&run.head_sha, main_sha) {
        return None;
    }
    if !matches!(run.event.as_str(), "schedule" | "workflow_dispatch") {
        details.push(format!(
            "Linux Candidate active run has event {}, expected schedule or workflow_dispatch",
            run.event
        ));
        reason_codes.push("candidate-active-run-unavailable".to_owned());
        return None;
    }
    if !matches!(
        run.status.as_str(),
        "in_progress" | "queued" | "pending" | "requested" | "waiting"
    ) {
        details.push(format!(
            "Linux Candidate active run has status {}, expected an active workflow status",
            run.status
        ));
        reason_codes.push("candidate-active-run-unavailable".to_owned());
        return None;
    }
    let previous_detail_count = details.len();
    let Some(created_at) = validate_timestamp(
        "Linux Candidate active run timestamp",
        &run.created_at_utc,
        timing.now,
        timing.future_skew,
        details,
    ) else {
        reason_codes.push("candidate-active-run-unavailable".to_owned());
        return None;
    };
    if details.len() != previous_detail_count {
        reason_codes.push("candidate-active-run-unavailable".to_owned());
        return None;
    }
    let age = timing.now.saturating_sub(created_at);
    if age > timing.max_active_run_age {
        details.push(format!(
            "Linux Candidate active run for current main is {age}s old, exceeding {}s limit",
            timing.max_active_run_age
        ));
        reason_codes.push("candidate-active-run-stale".to_owned());
        return None;
    }
    reason_codes.push("candidate-delivery-in-progress".to_owned());
    Some(format!(
        "Linux Candidate {} run for current main is {} ({age}s into {}s limit)",
        run.event, run.status, timing.max_active_run_age
    ))
}

fn validate_candidate_source(
    feed: &CandidateFeed,
    source: &SourceSnapshot,
    now: u64,
    max_lag: u64,
    future_skew: u64,
    details: &mut Vec<String>,
    summary: &mut String,
) -> Option<u64> {
    let evidence = &source.candidate;
    if let Some(error) = nonempty(evidence.error.as_deref()) {
        details.push(format!("Linux candidate source query failed: {error}"));
    }
    if !same_sha(&evidence.candidate_sha, &feed.commit_sha) {
        details.push(format!(
            "Linux candidate source evidence is for {}, expected {}",
            evidence.candidate_sha, feed.commit_sha
        ));
    }
    let candidate_committed_at = match evidence.candidate_committed_at_utc.as_deref() {
        Some(value) => validate_timestamp(
            "Linux candidate source commit timestamp",
            value,
            now,
            future_skew,
            details,
        ),
        None => {
            details.push("Linux candidate source commit timestamp is unavailable".to_owned());
            None
        }
    };

    match classify_candidate_source(evidence, &source.head_sha) {
        CandidateSourceRelation::Equal => {
            if !same_sha(&feed.commit_sha, &source.head_sha) {
                details.push(format!(
                    "Linux candidate source evidence matches current main {}, but the feed declares {}",
                    source.head_sha, feed.commit_sha
                ));
            }
            let age = candidate_committed_at
                .map(|_| {
                    parse_utc_timestamp(&feed.timestamp_utc)
                        .map(|timestamp| now.saturating_sub(timestamp))
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            *summary = format!(
                "Accepted Linux candidate {} build {} exactly matches current main; feed timestamp age {age}s is non-blocking",
                feed.version, feed.build
            );
        }
        CandidateSourceRelation::Ancestor => {
            if same_sha(&feed.commit_sha, &source.head_sha) {
                details.push(
                    "Linux candidate source is marked ancestor but it equals current main"
                        .to_owned(),
                );
            }
            match evidence
                .comparison
                .as_ref()
                .and_then(|comparison| comparison.first_unpublished_commit.as_ref())
            {
                None => details.push(
                    "Linux candidate is behind main but the first unpublished commit is unavailable"
                        .to_owned(),
                ),
                Some(commit) => {
                    if !is_hex(&commit.sha, 40) || same_sha(&commit.sha, &feed.commit_sha) {
                        details.push(
                            "Linux first unpublished main commit has an invalid identity".to_owned(),
                        );
                    }
                    if let Some(unpublished_at) = validate_timestamp(
                        "Linux first unpublished main commit timestamp",
                        &commit.committed_at_utc,
                        now,
                        future_skew,
                        details,
                    ) {
                        if candidate_committed_at.is_some_and(|source_at| unpublished_at < source_at)
                        {
                            details.push(
                                "Linux first unpublished main commit predates the candidate source"
                                    .to_owned(),
                            );
                        }
                        let lag = now.saturating_sub(unpublished_at);
                        if lag > max_lag {
                            details.push(format!(
                                "Linux candidate is stale: unpublished main lag {lag}s exceeds {max_lag}s"
                            ));
                        }
                        *summary = format!(
                            "Accepted Linux candidate {} build {} is behind current main by {lag}s from first unpublished commit {}",
                            feed.version, feed.build, commit.sha
                        );
                    }
                }
            }
        }
        CandidateSourceRelation::NotAncestor => details.push(format!(
            "Linux candidate source {} is not an ancestor of current main {}{}",
            feed.commit_sha,
            source.head_sha,
            evidence
                .comparison
                .as_ref()
                .map_or_else(String::new, |comparison| {
                    format!(
                        " (comparison status {}, merge base {})",
                        comparison.status, comparison.merge_base_sha
                    )
                })
        )),
        CandidateSourceRelation::Unknown => {
            details.push("Linux candidate source relation to current main is unknown".to_owned())
        }
    }

    candidate_committed_at
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateSourceRelation {
    Equal,
    Ancestor,
    NotAncestor,
    Unknown,
}

fn classify_candidate_source(
    evidence: &CandidateSourceSnapshot,
    main_sha: &str,
) -> CandidateSourceRelation {
    if same_sha(&evidence.candidate_sha, main_sha) {
        return CandidateSourceRelation::Equal;
    }
    match evidence.comparison.as_ref() {
        Some(comparison)
            if comparison.status == "ahead"
                && same_sha(&comparison.merge_base_sha, &evidence.candidate_sha) =>
        {
            CandidateSourceRelation::Ancestor
        }
        Some(_) => CandidateSourceRelation::NotAncestor,
        None => CandidateSourceRelation::Unknown,
    }
}

fn validate_timestamp(
    label: &str,
    value: &str,
    now: u64,
    future_skew: u64,
    details: &mut Vec<String>,
) -> Option<u64> {
    match parse_utc_timestamp(value) {
        None => {
            details.push(format!("{label} is not valid UTC RFC 3339: {value}"));
            None
        }
        Some(timestamp) if timestamp > now.saturating_add(future_skew) => {
            details.push(format!(
                "{label} is {} seconds in the future",
                timestamp - now
            ));
            Some(timestamp)
        }
        Some(timestamp) => Some(timestamp),
    }
}

fn validate_candidate_package(feed: &CandidateFeed, feed_url: &str, details: &mut Vec<String>) {
    let asset_base = feed_url
        .strip_suffix("candidate.linux.json")
        .filter(|base| is_https_url(base));
    if asset_base.is_none() {
        details.push(
            "Linux candidate feed URL must be absolute HTTPS and end in candidate.linux.json"
                .to_owned(),
        );
    }
    if feed.appimage.package_id != LINUX_VELOPACK_PACKAGE_ID {
        details.push(format!(
            "Linux candidate AppImage package id is {}, expected {}",
            feed.appimage.package_id, LINUX_VELOPACK_PACKAGE_ID
        ));
    }

    let expected_deb = format!("ok-player_{}_amd64.deb", feed.version);
    let expected_full = format!(
        "{}-{}-linux-candidate-full.nupkg",
        LINUX_VELOPACK_PACKAGE_ID, feed.version
    );
    for (label, name, url, size, expected_name) in [
        (
            "Debian",
            feed.package.name.as_str(),
            feed.package.url.as_str(),
            feed.package.size.unwrap_or_default(),
            expected_deb.as_str(),
        ),
        (
            "AppImage Full",
            feed.appimage.name.as_str(),
            feed.appimage.url.as_str(),
            feed.appimage.size,
            expected_full.as_str(),
        ),
    ] {
        if name != expected_name {
            details.push(format!(
                "Linux candidate {label} package name is {name}, expected {expected_name}"
            ));
        }
        let expected_url = asset_base.map(|base| format!("{base}{expected_name}"));
        if expected_url.as_deref() != Some(url) {
            details.push(format!("Linux candidate {label} package URL is not exact"));
        }
        if size == 0 {
            details.push(format!("Linux candidate {label} package size is zero"));
        }
    }

    let expected_sums_name = format!("SHA256SUMS-{}.txt", feed.build);
    let expected_sums_url = asset_base.map(|base| format!("{base}{expected_sums_name}"));
    if expected_sums_url.as_deref() != feed.sha256sums_url.as_deref() {
        details.push(format!(
            "Linux candidate checksum URL is not exact for {expected_sums_name}"
        ));
    }
}

fn validate_candidate_monotonicity(feed: &CandidateFeed, details: &mut Vec<String>) {
    let version_build = feed
        .version
        .rsplit('.')
        .next()
        .and_then(|part| part.parse::<u64>().ok());
    if feed.build == 0 || version_build != Some(feed.build) {
        details.push(format!(
            "Linux candidate version {} does not encode monotonic build {}",
            feed.version, feed.build
        ));
    }
    let mut previous_build = feed.build;
    let mut previous_version = feed.version.as_str();
    for entry in &feed.history {
        if entry.build >= previous_build
            || compare_versions(&entry.version, previous_version) != Ordering::Less
        {
            details.push(format!(
                "Linux candidate history is not strictly descending at {} build {}",
                entry.version, entry.build
            ));
            break;
        }
        previous_build = entry.build;
        previous_version = &entry.version;
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    published_at: Option<String>,
}

fn evaluate_stable_release(fetch: &FetchSnapshot, now: u64) -> HealthCheck {
    let fetch_errors = fetch_failure(fetch, "Stable Linux release query");
    if let Some(error) = fetch_errors.first() {
        return HealthCheck {
            name: "linux-stable-release-cadence".to_owned(),
            blocking: false,
            status: HealthStatus::Unknown,
            summary: error.clone(),
            details: Vec::new(),
            reason_codes: Vec::new(),
        };
    }
    let releases =
        match serde_json::from_str::<Vec<GithubRelease>>(fetch.body.as_deref().unwrap_or_default())
        {
            Ok(releases) => releases,
            Err(error) => {
                return HealthCheck {
                    name: "linux-stable-release-cadence".to_owned(),
                    blocking: false,
                    status: HealthStatus::Unknown,
                    summary: format!("Stable Linux release query is malformed: {error}"),
                    details: Vec::new(),
                    reason_codes: Vec::new(),
                };
            }
        };
    let mut linux_releases = releases
        .iter()
        .filter(|release| !release.draft && release.tag_name.starts_with("linux-v"))
        .map(|release| {
            release
                .published_at
                .as_deref()
                .and_then(parse_utc_timestamp)
                .map(|timestamp| (release, timestamp))
        });
    let Some((latest, timestamp)) = linux_releases.by_ref().flatten().max_by_key(|(_, at)| *at)
    else {
        return HealthCheck {
            name: "linux-stable-release-cadence".to_owned(),
            blocking: false,
            status: HealthStatus::Unknown,
            summary: "No published permanent linux-v* release with a valid UTC timestamp was found"
                .to_owned(),
            details: Vec::new(),
            reason_codes: Vec::new(),
        };
    };
    let age = now.saturating_sub(timestamp);
    let status = if age <= STABLE_RELEASE_FRESH_SECONDS {
        HealthStatus::Pass
    } else {
        HealthStatus::Warning
    };
    HealthCheck {
        name: "linux-stable-release-cadence".to_owned(),
        blocking: false,
        status,
        summary: format!(
            "Stable Linux release {} is {age}s old (diagnostic only)",
            latest.tag_name
        ),
        details: Vec::new(),
        reason_codes: Vec::new(),
    }
}

fn fetch_failure(fetch: &FetchSnapshot, label: &str) -> Vec<String> {
    if let Some(error) = nonempty(fetch.error.as_deref()) {
        vec![format!("{label} is unreachable: {error}")]
    } else if fetch.body.is_none() {
        vec![format!("{label} is unreachable: no response body")]
    } else {
        Vec::new()
    }
}

fn missing_candidate_identity(value: &Value) -> Vec<&'static str> {
    let required = [
        ("commit_sha", &["commit_sha"][..]),
        ("package.name", &["package", "name"][..]),
        ("package.url", &["package", "url"][..]),
        ("package.sha256", &["package", "sha256"][..]),
        ("appimage.package_id", &["appimage", "package_id"][..]),
        ("appimage.name", &["appimage", "name"][..]),
        ("appimage.url", &["appimage", "url"][..]),
        ("appimage.sha256", &["appimage", "sha256"][..]),
        ("sha256sums_url", &["sha256sums_url"][..]),
    ];
    required
        .into_iter()
        .filter_map(|(name, path)| {
            let field = path.iter().try_fold(value, |current, key| current.get(key));
            match field {
                Some(Value::String(text)) if !text.trim().is_empty() => None,
                _ => Some(name),
            }
        })
        .collect()
}

fn check(name: &str, blocking: bool, details: Vec<String>, passing_summary: String) -> HealthCheck {
    check_with_reason_codes(name, blocking, details, passing_summary, Vec::new())
}

fn check_with_reason_codes(
    name: &str,
    blocking: bool,
    details: Vec<String>,
    passing_summary: String,
    reason_codes: Vec<String>,
) -> HealthCheck {
    let status = if details.is_empty() {
        HealthStatus::Pass
    } else {
        HealthStatus::Fail
    };
    let summary = details.first().cloned().unwrap_or(passing_summary);
    HealthCheck {
        name: name.to_owned(),
        blocking,
        status,
        summary,
        details,
        reason_codes,
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

fn same_sha(left: &str, right: &str) -> bool {
    is_hex(left, 40) && is_hex(right, 40) && left.eq_ignore_ascii_case(right)
}

fn is_https_url(value: &str) -> bool {
    value.starts_with("https://") && value.len() > "https://".len()
}

pub fn parse_utc_timestamp(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let year = parse_digits(&bytes[0..4])? as i64;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;
    if year < 1970
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + u64::from(hour * 3_600 + minute * 60 + second))
}

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.iter().all(u8::is_ascii_digit) {
        bytes.iter().try_fold(0_u32, |value, digit| {
            value.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
        })
    } else {
        None
    }
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<u64> {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let unix_days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(unix_days).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strict_utc_timestamp() {
        assert_eq!(parse_utc_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_utc_timestamp("2026-07-18T01:00:47Z"),
            Some(1_784_336_447)
        );
        assert_eq!(parse_utc_timestamp("2026-02-29T00:00:00Z"), None);
        assert_eq!(parse_utc_timestamp("2026-07-18T01:00:47+00:00"), None);
    }
}
