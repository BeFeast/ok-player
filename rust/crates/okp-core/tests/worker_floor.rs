#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;

#[test]
fn healthy_floor_cleans_holds_skips_claims_and_spawns_the_oldest_eligible_issue() {
    let root = unique_temp_dir("okp-worker-floor-healthy");
    let harness = Harness::new(root.path());
    fs::create_dir_all(harness.source.join(".claude")).expect("agent junk root");
    fs::write(harness.source.join(".claude/session.json"), b"{}\n").expect("agent junk");
    fs::write(
        &harness.fleet,
        br#"{
  "projects": [{
    "name": "ok-player",
    "live_workers": 0,
    "paused": false,
    "outcome": {"health_state": "healthy"},
    "issue_claims": [
      247,
      {"issue_number": 248},
      {"issue_number": 251, "kind": 42, "status": [], "session": "", "pr_number": "unknown"},
      {"issue": 252, "kind": "terminal_reconciliation", "session": "ok-player-legacy", "pr_number": 900, "status": "done"}
    ]
  }]
}
"#,
    )
    .expect("fleet fixture");
    fs::write(
        &harness.issues,
        br#"[
  {"number": 249, "createdAt": "2026-07-22T00:05:00Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 545, "createdAt": "2026-07-22T00:00:00Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 246, "createdAt": "2026-07-22T00:01:00Z", "labels": [{"name": "ok-player-ready"}, {"name": "blocked"}]},
  {"number": 247, "createdAt": "2026-07-22T00:02:00Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 248, "createdAt": "2026-07-22T00:03:00Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 251, "createdAt": "2026-07-22T00:04:00Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 252, "createdAt": "2026-07-22T00:04:30Z", "labels": [{"name": "ok-player-ready"}]},
  {"number": 250, "createdAt": "2026-07-22T00:06:00Z", "labels": [{"name": "ok-player-ready"}]}
]
"#,
    )
    .expect("issue fixture");

    let output = harness.run(
        "249",
        r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
        r#"{"state":"CLOSED","labels":[{"name":"ok-player-ready"}]}"#,
        r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("removed ok-player-ready from active QA-hold issue #545"));
    assert!(stdout.contains("issue #545 is on active QA hold"));
    assert!(stdout.contains("issue #247 already has a Maestro claim"));
    assert!(stdout.contains("issue #248 already has a Maestro claim"));
    assert!(stdout.contains("issue #251 already has a Maestro claim"));
    assert!(stdout.contains("issue #252 already has a Maestro claim"));
    assert!(stdout.contains("quarantined agent-junk root .claude"));
    assert!(stdout.contains("spawned issue #249"));

    let log = fs::read_to_string(&harness.log).expect("command log");
    let hold_edit = log.find("issue edit 545").expect("hold label edit");
    let queue_read = log.find("issue list").expect("ready queue read");
    let spawn = log.find("maestro spawn").expect("Maestro spawn");
    assert!(hold_edit < queue_read && queue_read < spawn, "{log}");
    assert!(log.contains("--issue 249"), "{log}");
    assert!(!log.contains("issue edit 546"), "{log}");
    assert!(!log.contains("pr view 900"), "{log}");
    assert!(!log.contains("greptile"), "{log}");
    assert!(!log.contains("max_parallel"), "{log}");

    assert!(!harness.source.join(".claude").exists());
    let quarantined = fs::read_dir(&harness.quarantine)
        .expect("quarantine directory")
        .map(|entry| entry.expect("quarantine entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(quarantined.len(), 1);
    assert!(quarantined[0].to_string_lossy().starts_with("claude-"));
    assert!(git_status(&harness.source).is_empty());
}

#[test]
fn pause_health_and_live_worker_gates_still_clean_hold_labels_before_returning() {
    for (case_name, live_workers, paused, health, expected) in [
        ("paused", 0, true, "healthy", "project is paused; no spawn"),
        (
            "unhealthy",
            0,
            false,
            "failing",
            "project outcome is failing; no spawn",
        ),
        (
            "live",
            2,
            false,
            "healthy",
            "project already has 2 live worker(s)",
        ),
    ] {
        let root = unique_temp_dir(&format!("okp-worker-floor-{case_name}"));
        let harness = Harness::new(root.path());
        fs::create_dir_all(harness.source.join(".cursor")).expect("agent junk root");
        fs::write(harness.source.join(".cursor/cache"), b"fixture\n").expect("agent junk");
        fs::write(
            &harness.fleet,
            format!(
                "{{\"projects\":[{{\"name\":\"ok-player\",\"live_workers\":{live_workers},\"paused\":{paused},\"outcome\":{{\"health_state\":\"{health}\"}},\"issue_claims\":[]}}]}}\n"
            ),
        )
        .expect("fleet fixture");
        fs::write(&harness.issues, b"[]\n").expect("unused issue fixture");

        let output = harness.run(
            "999",
            r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
            r#"{"state":"OPEN","labels":[]}"#,
            r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
        );
        assert!(
            output.status.success(),
            "{case_name}: stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("removed ok-player-ready from active QA-hold issue #545"),
            "{case_name}: {stdout}"
        );
        assert!(stdout.contains(expected), "{case_name}: {stdout}");

        let log = fs::read_to_string(&harness.log).expect("command log");
        assert!(log.contains("issue edit 545"), "{case_name}: {log}");
        assert!(!log.contains("issue list"), "{case_name}: {log}");
        assert!(!log.contains("maestro spawn"), "{case_name}: {log}");
        assert!(harness.source.join(".cursor/cache").is_file());
        assert!(!harness.quarantine.exists());
    }
}

#[test]
fn quarantine_preserves_unknown_dirty_changes_and_refuses_to_spawn() {
    let root = unique_temp_dir("okp-worker-floor-dirty-source");
    let harness = Harness::new(root.path());
    fs::create_dir_all(harness.source.join(".agents")).expect("agent junk root");
    fs::write(harness.source.join(".agents/state"), b"fixture\n").expect("agent junk");
    fs::write(harness.source.join("README.md"), b"operator change\n")
        .expect("tracked operator change");
    fs::write(
        &harness.fleet,
        br#"{"projects":[{"name":"ok-player","live_workers":0,"paused":false,"outcome":{"health_state":"healthy"},"issue_claims":[]}]}
"#,
    )
    .expect("fleet fixture");
    fs::write(
        &harness.issues,
        br#"[{"number":300,"createdAt":"2026-07-22T00:00:00Z","labels":[{"name":"ok-player-ready"}]}]
"#,
    )
    .expect("issue fixture");

    let output = harness.run(
        "300",
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("canonical source checkout remains dirty after allowlisted quarantine")
    );
    assert!(!harness.source.join(".agents").exists());
    assert_eq!(
        fs::read_to_string(harness.source.join("README.md")).expect("tracked change"),
        "operator change\n"
    );
    assert!(git_status(&harness.source).contains("README.md"));
    assert_eq!(
        fs::read_dir(&harness.quarantine)
            .expect("quarantine directory")
            .count(),
        1
    );
    let log = fs::read_to_string(&harness.log).expect("command log");
    assert!(!log.contains("maestro spawn"), "{log}");
}

#[test]
fn final_fleet_refresh_stops_when_another_worker_wins_the_race() {
    let root = unique_temp_dir("okp-worker-floor-race");
    let harness = Harness::new(root.path());
    fs::create_dir_all(harness.source.join(".claude")).expect("agent junk root");
    fs::write(harness.source.join(".claude/state"), b"fixture\n").expect("agent junk");
    fs::write(
        &harness.fleet,
        br#"{"projects":[{"name":"ok-player","live_workers":0,"paused":false,"outcome":{"health_state":"healthy"},"issue_claims":[]}]}
"#,
    )
    .expect("initial fleet fixture");
    fs::write(
        &harness.second_fleet,
        br#"{"projects":[{"name":"ok-player","live_workers":1,"paused":false,"outcome":{"health_state":"healthy"},"issue_claims":[301]}]}
"#,
    )
    .expect("second fleet fixture");
    fs::write(
        &harness.issues,
        br#"[{"number":301,"createdAt":"2026-07-22T00:00:00Z","labels":[{"name":"ok-player-ready"}]}]
"#,
    )
    .expect("issue fixture");

    let output = harness.run(
        "301",
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("project already has 1 live worker(s)")
    );
    assert!(harness.source.join(".claude/state").is_file());
    assert!(!harness.quarantine.exists());
    let log = fs::read_to_string(&harness.log).expect("command log");
    assert!(!log.contains("maestro spawn"), "{log}");
}

#[test]
fn merged_claims_close_open_issues_and_stop_exact_ghost_sessions() {
    let root = unique_temp_dir("okp-worker-floor-merged-ghosts");
    let harness = Harness::new(root.path());
    fs::write(
        &harness.fleet,
        br#"{
  "projects": [{
    "name": "ok-player",
    "live_workers": 0,
    "paused": false,
    "outcome": {"health_state": "healthy"},
    "issue_claims": [
      {"issue_number": 439, "kind": "terminal_reconciliation", "session": "ok-player-439", "pr_number": 501, "status": "done"},
      {"issue_number": 340, "kind": "operator_gate", "session": "ok-player-340", "pr_number": 502, "status": "pr_open"},
      {"issue_number": 198, "kind": "terminal_reconciliation", "session": "ok-player-198", "pr_number": 503, "status": "done"},
      {"issue_number": 339, "kind": "open_pr_maintenance", "session": "ok-player-339", "pr_number": 504, "status": "retry_exhausted"},
      {"issue_number": 777, "kind": "open_pr_maintenance", "session": "ok-player-777", "pr_number": 505, "status": "pr_open"}
    ]
  }]
}
"#,
    )
    .expect("fleet fixture");
    fs::write(
        &harness.second_fleet,
        br#"{
  "projects": [{
    "name": "ok-player",
    "live_workers": 0,
    "paused": false,
    "outcome": {"health_state": "healthy"},
    "issue_claims": [
      {"issue_number": 777, "kind": "open_pr_maintenance", "session": "ok-player-777", "pr_number": 505, "status": "pr_open"}
    ]
  }]
}
"#,
    )
    .expect("post-cleanup fleet fixture");
    fs::write(&harness.issues, b"[]\n").expect("empty ready queue");
    fs::write(
        &harness.prs,
        br#"{
  "501": {"state": "MERGED", "mergedAt": "2026-07-22T20:01:00Z", "body": "Refs #439", "closingIssuesReferences": []},
  "502": {"state": "MERGED", "mergedAt": "2026-07-22T20:02:00Z", "body": "Refs #340", "closingIssuesReferences": []},
  "503": {"state": "MERGED", "mergedAt": "2026-07-22T20:03:00Z", "body": "Refs #198", "closingIssuesReferences": []},
  "504": {"state": "MERGED", "mergedAt": "2026-07-22T20:04:00Z", "body": "Refs #339", "closingIssuesReferences": []},
  "505": {"state": "OPEN", "mergedAt": null, "body": "Refs #777", "closingIssuesReferences": []}
}
"#,
    )
    .expect("PR fixture");
    fs::write(
        &harness.issue_states,
        br#"{
  "439": {"state": "OPEN"},
  "340": {"state": "OPEN"},
  "198": {"state": "CLOSED"},
  "339": {"state": "OPEN"}
}
"#,
    )
    .expect("issue state fixture");

    let output = harness.run(
        "999",
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"OPEN","labels":[{"name":"ok-player-ready"}]}"#,
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("issue #198 is already closed after PR #503 merged"));
    for (issue, pr) in [(339, 504), (340, 502), (439, 501)] {
        assert!(
            stdout.contains(&format!("closed issue #{issue} after PR #{pr} merged")),
            "{stdout}"
        );
    }
    assert!(stdout.contains("merged ghost session ok-player-198 for issue #198 is already absent"));
    for issue in [339, 340, 439] {
        assert!(
            stdout.contains(&format!(
                "stopped merged ghost session ok-player-{issue} for issue #{issue}"
            )),
            "{stdout}"
        );
    }

    let log = fs::read_to_string(&harness.log).expect("command log");
    for pr in 501..=505 {
        assert!(log.contains(&format!("pr view {pr}")), "{log}");
    }
    for issue in [339, 340, 439] {
        assert!(log.contains(&format!("issue close {issue}")), "{log}");
    }
    assert!(!log.contains("issue close 198"), "{log}");
    assert!(!log.contains("issue close 777"), "{log}");
    for issue in [198, 339, 340, 439] {
        assert!(
            log.contains(&format!(
                "maestro stop --config {} --session ok-player-{issue}",
                harness.config.display()
            )),
            "{log}"
        );
    }
    assert!(!log.contains("--session ok-player-777"), "{log}");
    let last_stop = log
        .rfind("maestro stop")
        .expect("at least one ghost session stop");
    let queue_read = log.find("issue list").expect("ready queue read");
    assert!(last_stop < queue_read, "{log}");
    assert!(!log.contains("maestro spawn"), "{log}");
}

#[test]
fn mismatched_pr_link_fails_before_issue_or_session_mutation() {
    let root = unique_temp_dir("okp-worker-floor-mismatched-pr-link");
    let harness = Harness::new(root.path());
    fs::write(
        &harness.fleet,
        br#"{
  "projects": [{
    "name": "ok-player",
    "live_workers": 0,
    "paused": false,
    "outcome": {"health_state": "healthy"},
    "issue_claims": [
      {"issue_number": 900, "kind": "terminal_reconciliation", "session": "ok-player-900", "pr_number": 42, "status": "done"}
    ]
  }]
}
"#,
    )
    .expect("fleet fixture");
    fs::write(
        &harness.prs,
        br#"{"42":{"state":"MERGED","mergedAt":"2026-07-22T20:00:00Z","body":"Refs #123","closingIssuesReferences":[]}}
"#,
    )
    .expect("PR fixture");
    fs::write(&harness.issues, b"[]\n").expect("unused issue fixture");

    let output = harness.run(
        "999",
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("PR #42 does not link claimed issue #900")
    );
    let log = fs::read_to_string(&harness.log).expect("command log");
    assert!(log.contains("pr view 42"), "{log}");
    assert!(!log.contains("issue view 900"), "{log}");
    assert!(!log.contains("issue close 900"), "{log}");
    assert!(!log.contains("--session ok-player-900"), "{log}");
    assert!(!log.contains("maestro spawn"), "{log}");
}

#[test]
fn conflicting_session_claims_fail_before_github_mutation() {
    let root = unique_temp_dir("okp-worker-floor-conflicting-session");
    let harness = Harness::new(root.path());
    fs::write(
        &harness.fleet,
        br#"{
  "projects": [{
    "name": "ok-player",
    "live_workers": 0,
    "paused": false,
    "outcome": {"health_state": "healthy"},
    "issue_claims": [
      {"issue_number": 601, "session": "ok-player-801", "pr_number": 701},
      {"issue_number": 602, "session": "ok-player-801", "pr_number": 702}
    ]
  }]
}
"#,
    )
    .expect("fleet fixture");
    fs::write(&harness.issues, b"[]\n").expect("unused issue fixture");

    let output = harness.run(
        "999",
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
        r#"{"state":"CLOSED","labels":[]}"#,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("conflicting claims for one Maestro session")
    );
    let log = fs::read_to_string(&harness.log).expect("command log");
    assert!(!log.contains("pr view"), "{log}");
    assert!(!log.contains("issue close"), "{log}");
    assert!(!log.contains("maestro stop"), "{log}");
    assert!(!log.contains("maestro spawn"), "{log}");
}

struct Harness {
    root: PathBuf,
    source: PathBuf,
    state: PathBuf,
    quarantine: PathBuf,
    config: PathBuf,
    fleet: PathBuf,
    second_fleet: PathBuf,
    issues: PathBuf,
    prs: PathBuf,
    issue_states: PathBuf,
    log: PathBuf,
    fake_bin: PathBuf,
}

impl Harness {
    fn new(root: &Path) -> Self {
        let source = root.join("source");
        let state = root.join("state");
        let quarantine = root.join("quarantine");
        let config = root.join("project.yaml");
        let fleet = root.join("fleet.json");
        let second_fleet = root.join("fleet-second.json");
        let issues = root.join("issues.json");
        let prs = root.join("prs.json");
        let issue_states = root.join("issue-states.json");
        let log = root.join("commands.log");
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin");
        fs::write(&config, b"project: fixture\n").expect("Maestro config fixture");
        fs::write(&prs, b"{}\n").expect("empty PR fixture");
        fs::write(&issue_states, b"{}\n").expect("empty issue state fixture");
        init_source_repository(&source);
        write_executable(&fake_bin.join("curl"), FAKE_CURL);
        write_executable(&fake_bin.join("gh"), FAKE_GH);
        write_executable(&fake_bin.join("maestro"), FAKE_MAESTRO);
        Self {
            root: root.to_path_buf(),
            source,
            state,
            quarantine,
            config,
            fleet,
            second_fleet,
            issues,
            prs,
            issue_states,
            log,
            fake_bin,
        }
    }

    fn run(
        &self,
        selected_issue: &str,
        hold_545: &str,
        hold_546: &str,
        selected_payload: &str,
    ) -> Output {
        let path = format!(
            "{}:{}",
            self.fake_bin.display(),
            std::env::var("PATH").expect("PATH")
        );
        let mut command = Command::new("bash");
        command
            .arg(repository_root().join("scripts/ok-player-worker-floor.sh"))
            .env("PATH", path)
            .env("FAKE_COMMAND_LOG", &self.log)
            .env("FAKE_FLEET_JSON", &self.fleet)
            .env("FAKE_FLEET_COUNT", self.root.join("fleet-count"))
            .env("FAKE_ISSUES_JSON", &self.issues)
            .env("FAKE_PRS_JSON", &self.prs)
            .env("FAKE_ISSUE_STATES_JSON", &self.issue_states)
            .env("FAKE_SELECTED_ISSUE", selected_issue)
            .env("FAKE_HOLD_545", hold_545)
            .env("FAKE_HOLD_546", hold_546)
            .env("FAKE_SELECTED_PAYLOAD", selected_payload)
            .env("OKP_WORKER_FLOOR_REPOSITORY", "BeFeast/ok-player")
            .env("OKP_WORKER_FLOOR_PROJECT", "ok-player")
            .env("OKP_WORKER_FLOOR_CONFIG", &self.config)
            .env("OKP_WORKER_FLOOR_FLEET_URL", "https://fleet.invalid/api")
            .env("OKP_WORKER_FLOOR_SOURCE_REPOSITORY", &self.source)
            .env("OKP_WORKER_FLOOR_STATE_DIR", &self.state)
            .env("OKP_WORKER_FLOOR_QUARANTINE_DIR", &self.quarantine)
            .env("OKP_WORKER_FLOOR_QA_HOLD_ISSUES", "545 546")
            .env(
                "OKP_WORKER_FLOOR_QUARANTINE_ROOTS",
                ".agents .claude .cursor",
            )
            .env("OKP_WORKER_FLOOR_MAESTRO_BIN", "maestro")
            .current_dir(&self.root);
        if self.second_fleet.is_file() {
            command.env("FAKE_SECOND_FLEET_JSON", &self.second_fleet);
        }
        command.output().expect("worker-floor script")
    }
}

fn init_source_repository(source: &Path) {
    fs::create_dir_all(source).expect("source repository");
    run_git(source, &["init", "--initial-branch=main"]);
    run_git(source, &["config", "user.name", "Worker Floor Test"]);
    run_git(
        source,
        &["config", "user.email", "worker-floor@example.invalid"],
    );
    run_git(source, &["config", "commit.gpgsign", "false"]);
    fs::write(source.join("README.md"), b"fixture\n").expect("tracked fixture");
    run_git(source, &["add", "README.md"]);
    run_git(source, &["commit", "-m", "fixture"]);
}

fn run_git(source: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(args)
        .output()
        .expect("git command");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_status(source: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .expect("git status");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("UTF-8 git status")
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write executable");
    let mut permissions = fs::metadata(path)
        .expect("executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable permissions");
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

const FAKE_GH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
printf 'gh' >>"$FAKE_COMMAND_LOG"
printf ' %s' "$@" >>"$FAKE_COMMAND_LOG"
printf '\n' >>"$FAKE_COMMAND_LOG"
args=" $* "
if [[ "$args" == *" issue view 545 "* ]]; then
  printf '%s\n' "$FAKE_HOLD_545"
elif [[ "$args" == *" issue view 546 "* ]]; then
  printf '%s\n' "$FAKE_HOLD_546"
elif [[ "$args" == *" issue edit 545 "* || "$args" == *" issue edit 546 "* ]]; then
  exit 0
elif [[ "$args" == *" issue list "* ]]; then
  cp -- "$FAKE_ISSUES_JSON" /dev/stdout
elif [[ "$args" == *" issue view ${FAKE_SELECTED_ISSUE} "* ]]; then
  printf '%s\n' "$FAKE_SELECTED_PAYLOAD"
elif [[ "$args" == *" pr view "* ]]; then
  next_is_number=false
  number=""
  for argument in "$@"; do
    if [[ "$next_is_number" == "true" ]]; then
      number="$argument"
      break
    fi
    if [[ "$argument" == "view" ]]; then
      next_is_number=true
    fi
  done
  jq -ce --arg number "$number" '.[$number] // error("missing PR fixture")' "$FAKE_PRS_JSON"
elif [[ "$args" == *" issue view "* ]]; then
  next_is_number=false
  number=""
  for argument in "$@"; do
    if [[ "$next_is_number" == "true" ]]; then
      number="$argument"
      break
    fi
    if [[ "$argument" == "view" ]]; then
      next_is_number=true
    fi
  done
  jq -ce --arg number "$number" '.[$number] // error("missing issue fixture")' "$FAKE_ISSUE_STATES_JSON"
elif [[ "$args" == *" issue close "* ]]; then
  exit 0
else
  echo "unexpected gh invocation: $*" >&2
  exit 90
fi
"#;

const FAKE_CURL: &str = r#"#!/usr/bin/env bash
set -euo pipefail
printf 'curl' >>"$FAKE_COMMAND_LOG"
printf ' %s' "$@" >>"$FAKE_COMMAND_LOG"
printf '\n' >>"$FAKE_COMMAND_LOG"
output=""
while (( $# > 0 )); do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
[[ -n "$output" ]] || { echo "missing curl output path" >&2; exit 91; }
count=0
if [[ -f "$FAKE_FLEET_COUNT" ]]; then
  read -r count <"$FAKE_FLEET_COUNT"
fi
(( count += 1 ))
printf '%s\n' "$count" >"$FAKE_FLEET_COUNT"
source="$FAKE_FLEET_JSON"
if (( count >= 2 )) && [[ -n "${FAKE_SECOND_FLEET_JSON:-}" ]]; then
  source="$FAKE_SECOND_FLEET_JSON"
fi
cp -- "$source" "$output"
"#;

const FAKE_MAESTRO: &str = r#"#!/usr/bin/env bash
set -euo pipefail
printf 'maestro' >>"$FAKE_COMMAND_LOG"
printf ' %s' "$@" >>"$FAKE_COMMAND_LOG"
printf '\n' >>"$FAKE_COMMAND_LOG"
if [[ " $* " == *" stop "* && " $* " == *" --session ok-player-198 "* ]]; then
  echo "session ok-player-198 not found" >&2
  exit 1
fi
"#;
