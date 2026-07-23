use std::fs;
use std::path::PathBuf;

use okp_core::project_health::{
    ActiveCandidateRunSnapshot, CandidateComparisonSnapshot, CandidateSourceSnapshot,
    CandidateWorkflowSnapshot, CommitSnapshot, FetchSnapshot, HealthStatus, ProjectHealthSnapshot,
    ScheduleRunSnapshot, SourceSnapshot, WorkflowSnapshot,
};
use serde::Deserialize;

const CANDIDATE_SHA: &str = "d5d531a58c830a01a7e25615e850593e9ff4493f";

#[derive(Deserialize)]
struct Case {
    name: String,
    feed_fixture: Option<String>,
    checked_at_unix: u64,
    fetch_error: Option<String>,
    main_sha: String,
    source_relation: FixtureSourceRelation,
    #[serde(default)]
    feed_timestamp_override: Option<String>,
    #[serde(default)]
    package_name_override: Option<String>,
    #[serde(default = "default_candidate_committed_at")]
    candidate_committed_at_utc: String,
    first_unpublished_sha: Option<String>,
    first_unpublished_at_utc: Option<String>,
    #[serde(default = "default_workflow_state")]
    workflow_state: String,
    #[serde(default = "default_schedule_completed_at")]
    schedule_completed_at_utc: String,
    #[serde(default = "default_max_schedule_age")]
    max_candidate_schedule_age_seconds: u64,
    #[serde(default)]
    consecutive_failed_runs: u64,
    #[serde(default)]
    last_failed_gate: Option<String>,
    expected_healthy: bool,
    expected_reason: String,
    #[serde(default)]
    expected_reason_code: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FixtureSourceRelation {
    Equal,
    Ancestor,
    NotAncestor,
}

fn default_candidate_committed_at() -> String {
    "2026-07-18T00:30:00Z".to_owned()
}

fn default_workflow_state() -> String {
    "active".to_owned()
}

fn default_schedule_completed_at() -> String {
    "2026-07-18T01:55:47Z".to_owned()
}

fn default_max_schedule_age() -> u64 {
    7_200
}

#[test]
fn candidate_delivery_outcomes_are_fixture_driven() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let cases: Vec<Case> = serde_json::from_str(
        &fs::read_to_string(fixture_dir.join("cases.json")).expect("read cases fixture"),
    )
    .expect("parse cases fixture");

    for case in cases {
        let mut candidate_body = case
            .feed_fixture
            .as_ref()
            .map(|name| fs::read_to_string(fixture_dir.join(name)).expect("read feed fixture"));
        if let (Some(body), Some(timestamp)) =
            (candidate_body.as_mut(), case.feed_timestamp_override)
        {
            let mut value: serde_json::Value =
                serde_json::from_str(body).expect("timestamp override requires valid JSON");
            value["timestamp_utc"] = serde_json::Value::String(timestamp);
            *body = serde_json::to_string(&value).expect("serialize overridden feed");
        }
        if let (Some(body), Some(name)) = (candidate_body.as_mut(), case.package_name_override) {
            let mut value: serde_json::Value =
                serde_json::from_str(body).expect("package override requires valid JSON");
            value["package"]["name"] = serde_json::Value::String(name.clone());
            value["package"]["url"] = serde_json::Value::String(format!(
                "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/{name}"
            ));
            *body = serde_json::to_string(&value).expect("serialize overridden feed");
        }
        let first_unpublished_commit =
            match (case.first_unpublished_sha, case.first_unpublished_at_utc) {
                (Some(sha), Some(committed_at_utc)) => Some(CommitSnapshot {
                    sha,
                    committed_at_utc,
                }),
                (None, None) => None,
                _ => panic!("{} has incomplete unpublished commit evidence", case.name),
            };
        let mut snapshot = healthy_snapshot(
            case.checked_at_unix,
            FetchSnapshot {
                url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
                body: candidate_body,
                error: case.fetch_error,
            },
            case.main_sha,
            candidate_source(
                case.source_relation,
                case.candidate_committed_at_utc,
                first_unpublished_commit,
            ),
        );
        snapshot.max_candidate_schedule_age_seconds = case.max_candidate_schedule_age_seconds;
        snapshot.source.candidate_workflow.state = case.workflow_state;
        snapshot.source.candidate_workflow.consecutive_failed_runs = case.consecutive_failed_runs;
        snapshot.source.candidate_workflow.last_failed_gate = case.last_failed_gate;
        snapshot
            .source
            .candidate_workflow
            .latest_completed_schedule
            .as_mut()
            .expect("healthy fixture has a completed schedule run")
            .completed_at_utc = case.schedule_completed_at_utc;
        let outcome = snapshot.evaluate();
        let candidate = outcome
            .checks
            .iter()
            .find(|check| check.name == "linux-candidate-delivery")
            .expect("candidate check");
        assert_eq!(outcome.healthy, case.expected_healthy, "{}", case.name);
        assert_eq!(
            candidate.status,
            if case.expected_healthy {
                HealthStatus::Pass
            } else {
                HealthStatus::Fail
            },
            "{}",
            case.name
        );
        assert!(
            candidate.summary.contains(&case.expected_reason)
                || candidate
                    .details
                    .iter()
                    .any(|detail| detail.contains(&case.expected_reason)),
            "{}: {:?}",
            case.name,
            candidate
        );
        if let Some(reason_code) = case.expected_reason_code {
            assert!(
                candidate.reason_codes.contains(&reason_code),
                "{}: {:?}",
                case.name,
                candidate
            );
        }
    }
}

#[test]
fn exact_main_candidate_run_bounds_stale_schedule_settling() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let candidate_feed = FetchSnapshot {
        url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
        body: Some(
            fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                .expect("read Linux candidate fixture"),
        ),
        error: None,
    };
    let main_sha = "1111111111111111111111111111111111111111";
    let mut snapshot = healthy_snapshot(
        1_784_340_047,
        candidate_feed,
        main_sha.to_owned(),
        candidate_source(
            FixtureSourceRelation::Ancestor,
            default_candidate_committed_at(),
            Some(CommitSnapshot {
                sha: "2222222222222222222222222222222222222222".to_owned(),
                committed_at_utc: "2026-07-18T01:30:47Z".to_owned(),
            }),
        ),
    );
    snapshot
        .source
        .candidate_workflow
        .latest_completed_schedule
        .as_mut()
        .expect("healthy fixture has schedule evidence")
        .completed_at_utc = "2026-07-17T23:00:47Z".to_owned();
    snapshot.source.candidate_workflow.latest_active_run = Some(ActiveCandidateRunSnapshot {
        head_sha: main_sha.to_owned(),
        event: "workflow_dispatch".to_owned(),
        status: "in_progress".to_owned(),
        created_at_utc: "2026-07-18T02:00:40Z".to_owned(),
        url: "https://example.invalid/run/active-candidate".to_owned(),
    });

    let outcome = snapshot.evaluate();
    let candidate = candidate_check(&outcome);
    assert!(outcome.healthy);
    assert_eq!(candidate.status, HealthStatus::Warning);
    assert!(candidate.summary.contains("workflow_dispatch run"));
    assert!(candidate.summary.contains("in_progress"));
    assert!(candidate.summary.contains("7s into 5400s limit"));
    assert!(
        candidate
            .reason_codes
            .contains(&"candidate-delivery-in-progress".to_owned())
    );

    let mut wrong_sha = snapshot.clone();
    wrong_sha
        .source
        .candidate_workflow
        .latest_active_run
        .as_mut()
        .expect("active run")
        .head_sha = "3333333333333333333333333333333333333333".to_owned();
    let wrong_sha_outcome = wrong_sha.evaluate();
    let wrong_sha_candidate = candidate_check(&wrong_sha_outcome);
    assert!(!wrong_sha_outcome.healthy);
    assert_eq!(wrong_sha_candidate.status, HealthStatus::Fail);
    assert!(
        wrong_sha_candidate
            .reason_codes
            .contains(&"candidate-schedule-stale".to_owned())
    );

    let mut stale_run = snapshot.clone();
    stale_run
        .source
        .candidate_workflow
        .latest_active_run
        .as_mut()
        .expect("active run")
        .created_at_utc = "2026-07-18T00:30:46Z".to_owned();
    let stale_run_outcome = stale_run.evaluate();
    let stale_run_candidate = candidate_check(&stale_run_outcome);
    assert!(!stale_run_outcome.healthy);
    assert_eq!(stale_run_candidate.status, HealthStatus::Fail);
    assert!(
        stale_run_candidate
            .reason_codes
            .contains(&"candidate-active-run-stale".to_owned())
    );
    assert!(
        stale_run_candidate
            .details
            .iter()
            .any(|detail| detail.contains("5401s old, exceeding 5400s limit"))
    );

    let mut unavailable_schedule = snapshot.clone();
    unavailable_schedule
        .source
        .candidate_workflow
        .schedule_error = Some("completed run query failed".to_owned());
    let unavailable_outcome = unavailable_schedule.evaluate();
    let unavailable_candidate = candidate_check(&unavailable_outcome);
    assert!(!unavailable_outcome.healthy);
    assert_eq!(unavailable_candidate.status, HealthStatus::Fail);
    assert!(
        unavailable_candidate
            .reason_codes
            .contains(&"candidate-schedule-unavailable".to_owned())
    );
    assert!(
        !unavailable_candidate
            .reason_codes
            .contains(&"candidate-delivery-in-progress".to_owned())
    );

    let mut failing_builds = snapshot;
    failing_builds
        .source
        .candidate_workflow
        .consecutive_failed_runs = 2;
    failing_builds.source.candidate_workflow.last_failed_gate =
        Some("headless-launch-smoke".to_owned());
    let failing_outcome = failing_builds.evaluate();
    let failing_candidate = candidate_check(&failing_outcome);
    assert!(!failing_outcome.healthy);
    assert_eq!(failing_candidate.status, HealthStatus::Fail);
    assert!(
        failing_candidate
            .reason_codes
            .contains(&"candidate-builds-failing".to_owned())
    );
}

#[test]
fn successful_runs_do_not_count_as_delivery_while_the_feed_is_behind() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let candidate_feed = FetchSnapshot {
        url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
        body: Some(
            fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                .expect("read Linux candidate fixture"),
        ),
        error: None,
    };
    let main_sha = "1111111111111111111111111111111111111111";
    let mut snapshot = healthy_snapshot(
        1_784_340_047,
        candidate_feed,
        main_sha.to_owned(),
        candidate_source(
            FixtureSourceRelation::Ancestor,
            default_candidate_committed_at(),
            Some(CommitSnapshot {
                sha: "2222222222222222222222222222222222222222".to_owned(),
                committed_at_utc: "2026-07-18T01:30:47Z".to_owned(),
            }),
        ),
    );
    snapshot.source.candidate_workflow.successful_delivery_runs = vec![
        successful_candidate_run("2026-07-18T01:55:47Z", "newer"),
        successful_candidate_run("2026-07-18T01:40:47Z", "older"),
    ];

    let repeated_outcome = snapshot.evaluate();
    let repeated = candidate_check(&repeated_outcome);
    assert!(!repeated_outcome.healthy);
    assert_eq!(repeated.status, HealthStatus::Fail);
    assert!(
        repeated
            .reason_codes
            .contains(&"candidate-delivery-not-published".to_owned())
    );
    assert!(repeated.details.iter().any(|detail| {
        detail.contains("completed 2 successful runs within 7200s")
            && detail.contains("workflow success is non-delivery")
    }));

    let mut one_attempt = snapshot.clone();
    one_attempt
        .source
        .candidate_workflow
        .successful_delivery_runs
        .pop();
    let one_attempt_outcome = one_attempt.evaluate();
    let one_attempt_check = candidate_check(&one_attempt_outcome);
    assert!(one_attempt_outcome.healthy);
    assert_eq!(one_attempt_check.status, HealthStatus::Warning);
    assert!(
        one_attempt_check
            .summary
            .contains("recovery dispatch is required")
    );

    let mut active = snapshot.clone();
    active.source.candidate_workflow.latest_active_run = Some(ActiveCandidateRunSnapshot {
        head_sha: main_sha.to_owned(),
        event: "workflow_dispatch".to_owned(),
        status: "in_progress".to_owned(),
        created_at_utc: "2026-07-18T02:00:40Z".to_owned(),
        url: "https://example.invalid/run/active-candidate".to_owned(),
    });
    let active_outcome = active.evaluate();
    let active_check = candidate_check(&active_outcome);
    assert!(active_outcome.healthy);
    assert_eq!(active_check.status, HealthStatus::Warning);
    assert!(
        active_check
            .reason_codes
            .contains(&"candidate-delivery-in-progress".to_owned())
    );
    assert!(
        !active_check
            .reason_codes
            .contains(&"candidate-delivery-not-published".to_owned())
    );

    let mut overdue_active = active;
    overdue_active.checked_at_unix = 1_784_341_848;
    overdue_active
        .source
        .candidate
        .comparison
        .as_mut()
        .and_then(|comparison| comparison.first_unpublished_commit.as_mut())
        .expect("first unpublished commit")
        .committed_at_utc = "2026-07-18T00:00:47Z".to_owned();
    overdue_active
        .source
        .candidate_workflow
        .latest_active_run
        .as_mut()
        .expect("active run")
        .created_at_utc = "2026-07-18T02:30:40Z".to_owned();
    let overdue_outcome = overdue_active.evaluate();
    let overdue_check = candidate_check(&overdue_outcome);
    assert!(!overdue_outcome.healthy);
    assert_eq!(overdue_check.status, HealthStatus::Fail);
    assert!(
        overdue_check
            .details
            .iter()
            .any(|detail| detail.contains("unpublished main lag 9001s exceeds 7200s"))
    );
}

fn successful_candidate_run(completed_at_utc: &str, name: &str) -> ScheduleRunSnapshot {
    ScheduleRunSnapshot {
        head_sha: "1111111111111111111111111111111111111111".to_owned(),
        event: "workflow_dispatch".to_owned(),
        status: "completed".to_owned(),
        conclusion: "success".to_owned(),
        completed_at_utc: completed_at_utc.to_owned(),
        url: format!("https://example.invalid/run/{name}"),
    }
}

fn candidate_check(
    outcome: &okp_core::project_health::ProjectHealthOutcome,
) -> &okp_core::project_health::HealthCheck {
    outcome
        .checks
        .iter()
        .find(|check| check.name == "linux-candidate-delivery")
        .expect("candidate check")
}

#[test]
fn windows_candidate_delivery_reports_current_lag_bootstrap_and_gate_failures() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let candidate_feed = FetchSnapshot {
        url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
        body: Some(
            fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                .expect("read Linux candidate fixture"),
        ),
        error: None,
    };

    let current = healthy_snapshot(
        1_784_340_047,
        candidate_feed.clone(),
        CANDIDATE_SHA.to_owned(),
        candidate_source(
            FixtureSourceRelation::Equal,
            default_candidate_committed_at(),
            None,
        ),
    );
    let current_outcome = current.evaluate();
    let current_check = windows_candidate_check(&current_outcome);
    assert!(current_outcome.healthy);
    assert_eq!(current_check.status, HealthStatus::Pass);
    assert!(
        current_check
            .summary
            .contains("exactly matches current main")
    );

    let mut within_sla = healthy_snapshot(
        1_784_340_047,
        candidate_feed.clone(),
        "1111111111111111111111111111111111111111".to_owned(),
        candidate_source(
            FixtureSourceRelation::Ancestor,
            default_candidate_committed_at(),
            Some(CommitSnapshot {
                sha: "2222222222222222222222222222222222222222".to_owned(),
                committed_at_utc: "2026-07-18T01:30:47Z".to_owned(),
            }),
        ),
    );
    within_sla.source.windows_candidate = candidate_source(
        FixtureSourceRelation::Ancestor,
        default_candidate_committed_at(),
        Some(CommitSnapshot {
            sha: "2222222222222222222222222222222222222222".to_owned(),
            committed_at_utc: "2026-07-18T01:30:47Z".to_owned(),
        }),
    );
    let within_sla_outcome = within_sla.evaluate();
    let within_sla_check = windows_candidate_check(&within_sla_outcome);
    assert_eq!(within_sla_check.status, HealthStatus::Pass);
    assert!(
        within_sla_check
            .summary
            .contains("behind current main by 1800s")
    );

    let mut beyond_sla = within_sla.clone();
    beyond_sla.checked_at_unix = 1_784_341_848;
    beyond_sla
        .source
        .windows_candidate
        .candidate_committed_at_utc = Some("2026-07-17T23:30:00Z".to_owned());
    beyond_sla
        .source
        .windows_candidate
        .comparison
        .as_mut()
        .and_then(|comparison| comparison.first_unpublished_commit.as_mut())
        .expect("Windows candidate ancestor evidence")
        .committed_at_utc = "2026-07-18T00:00:47Z".to_owned();
    let beyond_sla_outcome = beyond_sla.evaluate();
    let beyond_sla_check = windows_candidate_check(&beyond_sla_outcome);
    assert_eq!(beyond_sla_check.status, HealthStatus::Fail);
    assert!(
        beyond_sla_check
            .summary
            .contains("unpublished main lag 9001s exceeds 7200s")
    );

    let mut failing = current.clone();
    failing
        .source
        .windows_candidate_workflow
        .consecutive_failed_runs = 3;
    failing.source.windows_candidate_workflow.last_failed_gate =
        Some("Run core unit tests".to_owned());
    let failing_outcome = failing.evaluate();
    let failing_check = windows_candidate_check(&failing_outcome);
    assert_eq!(failing_check.status, HealthStatus::Fail);
    assert_eq!(
        failing_check.summary,
        "Windows candidate builder failing at gate Run core unit tests (3 consecutive)"
    );
    assert!(
        failing_check
            .reason_codes
            .contains(&"windows-candidate-builds-failing".to_owned())
    );

    let mut tampered = current.clone();
    tampered
        .windows_candidate_feed
        .body
        .as_mut()
        .expect("Windows candidate feed fixture")
        .push(' ');
    let tampered_outcome = tampered.evaluate();
    let tampered_check = windows_candidate_check(&tampered_outcome);
    assert_eq!(tampered_check.status, HealthStatus::Fail);
    assert!(
        tampered_check
            .summary
            .contains("does not match its identity manifest")
    );

    let mut bootstrap = current;
    bootstrap
        .source
        .windows_candidate_workflow
        .latest_completed_schedule = None;
    bootstrap.windows_candidate_manifest.body = None;
    bootstrap.windows_candidate_manifest.error = Some("not published".to_owned());
    bootstrap.windows_candidate_feed.body = None;
    bootstrap.windows_candidate_feed.error = Some("not published".to_owned());
    let bootstrap_outcome = bootstrap.evaluate();
    let bootstrap_check = windows_candidate_check(&bootstrap_outcome);
    assert!(bootstrap_outcome.healthy);
    assert!(bootstrap_check.blocking);
    assert_eq!(bootstrap_check.status, HealthStatus::Warning);
    assert!(bootstrap_check.summary.contains("bootstrapping"));
}

fn windows_candidate_check(
    outcome: &okp_core::project_health::ProjectHealthOutcome,
) -> &okp_core::project_health::HealthCheck {
    outcome
        .checks
        .iter()
        .find(|check| check.name == "windows-candidate-delivery")
        .expect("Windows candidate check")
}

#[test]
fn source_main_ci_settling_is_bounded_and_completed_failures_are_immediate() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let candidate_feed = FetchSnapshot {
        url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
        body: Some(
            fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                .expect("read candidate fixture"),
        ),
        error: None,
    };
    let mut snapshot = healthy_snapshot(
        1_784_340_047,
        candidate_feed,
        CANDIDATE_SHA.to_owned(),
        candidate_source(
            FixtureSourceRelation::Equal,
            default_candidate_committed_at(),
            None,
        ),
    );
    snapshot.source.head_committed_at_utc = Some("2026-07-17T00:00:00Z".to_owned());
    snapshot.source.head_observed_at_utc = Some("2026-07-18T01:59:47Z".to_owned());
    snapshot.source.workflows[0].status = "in_progress".to_owned();
    snapshot.source.workflows[0].conclusion.clear();
    snapshot.source.workflows[1].head_sha = "1111111111111111111111111111111111111111".to_owned();

    let settling = snapshot.evaluate();
    let settling_check = source_check(&settling);
    assert!(settling.healthy);
    assert_eq!(settling_check.status, HealthStatus::Warning);
    assert!(
        settling_check
            .reason_codes
            .contains(&"source-main-ci-settling".to_owned())
    );
    assert!(settling_check.summary.contains("60s into 900s grace"));

    let mut overdue = snapshot.clone();
    overdue.checked_at_unix += overdue.source_ci_grace_seconds;
    let overdue_outcome = overdue.evaluate();
    let overdue_check = source_check(&overdue_outcome);
    assert!(!overdue_outcome.healthy);
    assert_eq!(overdue_check.status, HealthStatus::Fail);
    assert!(
        overdue_check
            .details
            .iter()
            .any(|detail| detail.contains("exceeding 900s grace"))
    );

    let mut failed = snapshot;
    failed.source.workflows[0].status = "completed".to_owned();
    failed.source.workflows[0].conclusion = "failure".to_owned();
    let failed_outcome = failed.evaluate();
    let failed_check = source_check(&failed_outcome);
    assert!(!failed_outcome.healthy);
    assert_eq!(failed_check.status, HealthStatus::Fail);
    assert!(
        failed_check
            .details
            .iter()
            .any(|detail| detail.contains("CI is completed/failure"))
    );

    let mut malformed = failed;
    malformed.source.workflows[0].status = "in_progress".to_owned();
    let malformed_outcome = malformed.evaluate();
    let malformed_check = source_check(&malformed_outcome);
    assert!(!malformed_outcome.healthy);
    assert_eq!(malformed_check.status, HealthStatus::Fail);
    assert!(
        malformed_check
            .details
            .iter()
            .any(|detail| detail.contains("unexpected status in_progress/failure"))
    );
}

fn source_check(
    outcome: &okp_core::project_health::ProjectHealthOutcome,
) -> &okp_core::project_health::HealthCheck {
    outcome
        .checks
        .iter()
        .find(|check| check.name == "source-main-ci")
        .expect("source/main check")
}

#[test]
fn old_permanent_linux_release_is_diagnostic_only() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let snapshot = healthy_snapshot(
        1_784_941_247,
        FetchSnapshot {
            url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
            body: Some(
                fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                    .expect("read candidate fixture"),
            ),
            error: None,
        },
        CANDIDATE_SHA.to_owned(),
        CandidateSourceSnapshot {
            candidate_sha: CANDIDATE_SHA.to_owned(),
            candidate_committed_at_utc: Some("2026-07-18T00:30:00Z".to_owned()),
            comparison: None,
            error: None,
        },
    );
    let outcome = snapshot.evaluate();
    let stable = outcome
        .checks
        .iter()
        .find(|check| check.name == "linux-stable-release-cadence")
        .expect("stable release check");
    assert!(outcome.healthy);
    assert!(!stable.blocking);
    assert_eq!(stable.status, HealthStatus::Warning);
}

#[test]
fn stable_release_diagnostic_selects_newest_published_linux_tag() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_health");
    let mut snapshot = healthy_snapshot(
        1_784_340_047,
        FetchSnapshot {
            url: "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json".to_owned(),
            body: Some(
                fs::read_to_string(fixture_dir.join("fresh-accepted.json"))
                    .expect("read candidate fixture"),
            ),
            error: None,
        },
        CANDIDATE_SHA.to_owned(),
        CandidateSourceSnapshot {
            candidate_sha: CANDIDATE_SHA.to_owned(),
            candidate_committed_at_utc: Some("2026-07-18T00:30:00Z".to_owned()),
            comparison: None,
            error: None,
        },
    );
    snapshot.stable_linux_releases.body = Some(
        r#"[
            {"tag_name":"linux-v0.1.0-old","draft":false,"published_at":"2026-07-14T00:00:00Z"},
            {"tag_name":"v0.10.14","draft":false,"published_at":"2026-07-18T00:00:00Z"},
            {"tag_name":"linux-v0.1.0-draft","draft":true,"published_at":"2026-07-17T00:00:00Z"},
            {"tag_name":"linux-v0.1.0-new","draft":false,"published_at":"2026-07-16T00:00:00Z"}
        ]"#
        .to_owned(),
    );

    let outcome = snapshot.evaluate();
    let stable = outcome
        .checks
        .iter()
        .find(|check| check.name == "linux-stable-release-cadence")
        .expect("stable release check");
    assert!(stable.summary.contains("linux-v0.1.0-new"));
}

fn candidate_source(
    relation: FixtureSourceRelation,
    candidate_committed_at_utc: String,
    first_unpublished_commit: Option<CommitSnapshot>,
) -> CandidateSourceSnapshot {
    let comparison = match relation {
        FixtureSourceRelation::Equal => None,
        FixtureSourceRelation::Ancestor => Some(CandidateComparisonSnapshot {
            status: "ahead".to_owned(),
            merge_base_sha: CANDIDATE_SHA.to_owned(),
            first_unpublished_commit,
        }),
        FixtureSourceRelation::NotAncestor => Some(CandidateComparisonSnapshot {
            status: "diverged".to_owned(),
            merge_base_sha: "4444444444444444444444444444444444444444".to_owned(),
            first_unpublished_commit: None,
        }),
    };
    CandidateSourceSnapshot {
        candidate_sha: CANDIDATE_SHA.to_owned(),
        candidate_committed_at_utc: Some(candidate_committed_at_utc),
        comparison,
        error: None,
    }
}

fn healthy_snapshot(
    checked_at_unix: u64,
    candidate_feed: FetchSnapshot,
    head_sha: String,
    candidate: CandidateSourceSnapshot,
) -> ProjectHealthSnapshot {
    ProjectHealthSnapshot {
        checked_at_unix,
        source_ci_grace_seconds: 900,
        max_unpublished_main_lag_seconds: 7_200,
        max_candidate_schedule_age_seconds: 7_200,
        max_candidate_run_age_seconds: 5_400,
        future_skew_seconds: 300,
        source: SourceSnapshot {
            head_sha: head_sha.clone(),
            head_committed_at_utc: Some("2026-07-18T00:30:00Z".to_owned()),
            head_observed_at_utc: Some("2026-07-18T00:30:02Z".to_owned()),
            workflows: ["CI", "Rust"]
                .into_iter()
                .map(|name| WorkflowSnapshot {
                    name: name.to_owned(),
                    head_sha: head_sha.clone(),
                    event: "push".to_owned(),
                    status: "completed".to_owned(),
                    conclusion: "success".to_owned(),
                    created_at_utc: "2026-07-18T00:30:02Z".to_owned(),
                    url: format!("https://example.invalid/runs/{name}"),
                })
                .collect(),
            candidate_workflow: CandidateWorkflowSnapshot {
                state: "active".to_owned(),
                state_error: None,
                latest_completed_schedule: Some(ScheduleRunSnapshot {
                    head_sha: head_sha.clone(),
                    event: "schedule".to_owned(),
                    status: "completed".to_owned(),
                    conclusion: "success".to_owned(),
                    completed_at_utc: "2026-07-18T01:55:47Z".to_owned(),
                    url: "https://example.invalid/runs/linux-candidate".to_owned(),
                }),
                schedule_error: None,
                latest_active_run: None,
                active_run_error: None,
                successful_delivery_runs: Vec::new(),
                delivery_runs_error: None,
                consecutive_failed_runs: 0,
                last_failed_gate: None,
            },
            candidate: candidate.clone(),
            windows_candidate_workflow: CandidateWorkflowSnapshot {
                state: "active".to_owned(),
                state_error: None,
                latest_completed_schedule: Some(ScheduleRunSnapshot {
                    head_sha: head_sha.clone(),
                    event: "schedule".to_owned(),
                    status: "completed".to_owned(),
                    conclusion: "success".to_owned(),
                    completed_at_utc: "2026-07-18T01:55:47Z".to_owned(),
                    url: "https://example.invalid/runs/windows-candidate".to_owned(),
                }),
                schedule_error: None,
                latest_active_run: None,
                active_run_error: None,
                successful_delivery_runs: Vec::new(),
                delivery_runs_error: None,
                consecutive_failed_runs: 0,
                last_failed_gate: None,
            },
            windows_candidate: candidate,
            error: None,
        },
        windows_feed: FetchSnapshot {
            url: "https://befeast.github.io/ok-player/updates/win/releases.win.json".to_owned(),
            body: Some(
                r#"{"Assets":[{"PackageId":"OkPlayer","Version":"0.10.14","Type":"Full","FileName":"https://github.com/BeFeast/ok-player/releases/download/v0.10.14/OkPlayer-0.10.14-full.nupkg","SHA256":"B6C45F3FDAD98FF02958A77C30DE0EFE2260AF518C392A01699F1397E9C70E80","Size":200597245}]}"#
                    .to_owned(),
            ),
            error: None,
        },
        windows_candidate_manifest: FetchSnapshot {
            url: "https://github.com/BeFeast/ok-player/releases/download/windows-candidate/candidate.windows.json".to_owned(),
            body: Some(
                fs::read_to_string(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/project_health/windows-candidate-manifest.json"),
                )
                .expect("read Windows candidate manifest fixture"),
            ),
            error: None,
        },
        windows_candidate_feed: FetchSnapshot {
            url: "https://github.com/BeFeast/ok-player/releases/download/windows-candidate/releases.win-candidate.json".to_owned(),
            body: Some(
                fs::read_to_string(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/project_health/windows-candidate-feed.json"),
                )
                .expect("read Windows candidate feed fixture"),
            ),
            error: None,
        },
        candidate_feed,
        stable_linux_releases: FetchSnapshot {
            url: "https://api.github.com/repos/BeFeast/ok-player/releases?per_page=100"
                .to_owned(),
            body: Some(
                r#"[{"tag_name":"linux-v0.1.0-linux-alpha.112","draft":false,"published_at":"2026-07-15T19:35:55Z"}]"#
                    .to_owned(),
            ),
            error: None,
        },
    }
}
