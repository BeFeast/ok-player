use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn linux_schedule_skips_a_superseded_sha_before_preflight_and_lock() {
    let workflow = workflow("release-linux-candidate.yml");
    let checkout = position(&workflow, "- uses: actions/checkout@v4");
    let supersession = position(&workflow, "- name: Skip superseded scheduled SHA");
    let preflight = position(&workflow, "- name: Preflight bundled-mpv toolchain");
    let lock = position(&workflow, "- name: Build lock coordinator");

    assert!(checkout < supersession);
    assert!(supersession < preflight);
    assert!(supersession < lock);
    assert!(workflow.contains("if: github.event_name == 'schedule'"));
    assert!(
        workflow
            .contains("git fetch --no-tags origin \"+refs/heads/main:refs/remotes/origin/main\"")
    );
    assert!(workflow.contains("superseded by ${current_main_sha}, skipping"));
    assert!(workflow.contains("OKP_CANDIDATE_SKIPPED_SUPERSEDED"));

    for step in [
        "Preflight bundled-mpv toolchain",
        "Build lock coordinator",
        "Build and publish exact native bundle",
        "Upload portability smoke failure evidence",
        "Reclaim candidate scratch",
    ] {
        assert!(
            step_block(&workflow, step)
                .contains("steps.supersession.outputs.should_run != 'false'"),
            "{step} must be skipped after a supersession decision"
        );
    }
}

#[test]
fn linux_manual_dispatch_republish_contract_is_unchanged() {
    let workflow = workflow("release-linux-candidate.yml");

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("republish_last_bundle:"));
    assert!(
        workflow.contains(
            "OKP_CANDIDATE_FORCE_REPUBLISH: ${{ inputs.republish_last_bundle || 'false' }}"
        )
    );
    assert!(workflow.contains("if: github.event_name == 'schedule'"));
    assert!(workflow.contains("if: steps.supersession.outputs.should_run != 'false'"));
}

#[test]
fn windows_automatic_runs_skip_a_superseded_sha_before_build_setup() {
    let workflow = workflow("release-windows-candidate.yml");
    let checkout = position(&workflow, "- uses: actions/checkout@v4");
    let supersession = position(
        &workflow,
        "- name: Verify checkout and skip superseded automatic SHA",
    );
    let setup = position(&workflow, "- uses: actions/setup-dotnet@v4");

    assert!(checkout < supersession);
    assert!(supersession < setup);
    assert!(workflow.contains("push:\n    branches:\n      - main"));
    assert!(workflow.contains("if ('${{ github.event_name }}' -in @('push', 'schedule'))"));
    assert!(
        workflow.contains("git fetch --no-tags origin '+refs/heads/main:refs/remotes/origin/main'")
    );
    assert!(workflow.contains("superseded by $currentMain, skipping"));
    assert!(workflow.contains("OKP_CANDIDATE_SKIPPED_SUPERSEDED"));

    assert!(
        action_block(&workflow, "actions/setup-dotnet@v4")
            .contains("steps.supersession.outputs.should_run != 'false'")
    );
    for step in [
        "Build candidate contract CLI",
        "Read rolling publication and coalesce unchanged main",
    ] {
        assert!(
            step_block(&workflow, step)
                .contains("steps.supersession.outputs.should_run != 'false'"),
            "{step} must be skipped after a supersession decision"
        );
    }
}

#[test]
fn windows_manual_dispatch_bypasses_automatic_supersession() {
    let workflow = workflow("release-windows-candidate.yml");

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("if ('${{ github.event_name }}' -in @('push', 'schedule'))"));
}

fn workflow(name: &str) -> String {
    fs::read_to_string(repository_root().join(".github/workflows").join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn position(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing workflow fragment: {needle}"))
}

fn step_block<'a>(workflow: &'a str, name: &str) -> &'a str {
    block_from(workflow, &format!("      - name: {name}"))
}

fn action_block<'a>(workflow: &'a str, action: &str) -> &'a str {
    block_from(workflow, &format!("      - uses: {action}"))
}

fn block_from<'a>(workflow: &'a str, start: &str) -> &'a str {
    let start = position(workflow, start);
    let tail = &workflow[start..];
    let end = tail[1..]
        .find("\n      - ")
        .map_or(tail.len(), |offset| offset + 1);
    &tail[..end]
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
