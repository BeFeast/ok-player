use std::fs;
use std::path::PathBuf;

use okp_core::project_health::{
    CandidateComparisonSnapshot, CandidateSourceSnapshot, CandidateWorkflowSnapshot,
    CommitSnapshot, FetchSnapshot, HealthStatus, ProjectHealthSnapshot, ScheduleRunSnapshot,
    SourceSnapshot, WorkflowSnapshot,
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
    2_700
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
        max_unpublished_main_lag_seconds: 7_200,
        max_candidate_schedule_age_seconds: 2_700,
        future_skew_seconds: 300,
        source: SourceSnapshot {
            head_sha: head_sha.clone(),
            workflows: ["CI", "Rust"]
                .into_iter()
                .map(|name| WorkflowSnapshot {
                    name: name.to_owned(),
                    head_sha: head_sha.clone(),
                    event: "push".to_owned(),
                    status: "completed".to_owned(),
                    conclusion: "success".to_owned(),
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
            },
            candidate,
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
