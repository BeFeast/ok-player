//! Behavioural tests for the unfinished-work merge gate.
//!
//! These run the real check scripts against constructed pull request bodies and
//! constructed source trees and assert on exit codes and messages. They are not
//! source greps: revert either script's logic and these fail.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root should resolve")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// Declaration checks: scripts/check-unfinished-work.py
// ---------------------------------------------------------------------------

struct Declaration {
    root: TempDir,
}

impl Declaration {
    fn new(name: &str) -> Self {
        Self {
            root: unique_temp_dir(name),
        }
    }

    fn check(&self, title: &str, body: &str) -> Output {
        let title_file = self.root.path().join("title.txt");
        let body_file = self.root.path().join("body.txt");
        fs::write(&title_file, title).expect("title fixture should be written");
        fs::write(&body_file, body).expect("body fixture should be written");
        Command::new("python3")
            .arg(repo_root().join("scripts/check-unfinished-work.py"))
            .arg("--root")
            .arg(self.root.path())
            .arg("--title-file")
            .arg(&title_file)
            .arg("--body-file")
            .arg(&body_file)
            .output()
            .expect("unfinished work check should run")
    }
}

const FINISHED_BODY: &str = "## What this changes\n\nFixes the seek readout.\n";

#[test]
fn a_wip_marker_in_the_body_blocks_the_merge_and_removing_it_unblocks() {
    let fixture = Declaration::new("okp-gate-wip-marker");

    let blocked = fixture.check(
        "Fix Linux fullscreen screenshot surface restore",
        "## What this changes\n\nRestores the surface.\n\n<!-- maestro:wip -->\n",
    );
    assert_eq!(blocked.status.code(), Some(1));
    assert!(stderr_of(&blocked).contains("WIP marker"));

    let allowed = fixture.check(
        "Fix Linux fullscreen screenshot surface restore",
        FINISHED_BODY,
    );
    assert_eq!(allowed.status.code(), Some(0));
}

#[test]
fn a_wip_title_prefix_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-wip-title");

    for title in [
        "WIP: Reveal saved Linux screenshots in the file manager",
        "[WIP] Reveal saved Linux screenshots",
        "wip reveal saved Linux screenshots",
        "Draft: reveal saved Linux screenshots",
        "DO NOT MERGE - reveal saved Linux screenshots",
        // Standalone declarations, with nothing after them to delimit.
        "Draft",
        "Do not merge",
        "DNM",
    ] {
        let output = fixture.check(title, FINISHED_BODY);
        assert_eq!(output.status.code(), Some(1), "title should block: {title}");
        assert!(stderr_of(&output).contains("unfinished-work prefix"));
    }
}

#[test]
fn a_title_that_merely_starts_with_the_word_draft_is_not_blocked() {
    let fixture = Declaration::new("okp-gate-title-false-positive");

    let output = fixture.check(
        "Draft the Linux release notes from the tag range",
        FINISHED_BODY,
    );

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn an_unchecked_operator_acceptance_box_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance");
    let body = "## What this changes\n\nAdds dual-display handling.\n\n\
        ## Operator acceptance\n\n\
        - [x] Unit suite green\n\
        - [ ] Packaged build passes GNOME/Wayland dual-display QA\n";

    let output = fixture.check("Handle dual displays on Wayland", body);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("Operator acceptance is not complete"));
    assert!(stderr.contains("dual-display QA"));
}

#[test]
fn a_prose_operator_acceptance_hold_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance-prose");
    // The shape every real acceptance hold in this repository has taken: plain
    // bullets, so nothing can ever record that they were performed.
    let body = "## Verification\n\nHeadless only.\n\n\
        ## Operator acceptance hold\n\n\
        Do not mark ready until a packaged build passes real dual-display QA:\n\n\
        - at least 20 fullscreen cycles\n\
        - zero stale black planes\n";

    let output = fixture.check("Fix Linux fullscreen screenshot surface restore", body);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("cannot be resolved"));
    assert!(stderr.contains("at least 20 fullscreen cycles"));
}

#[test]
fn an_acceptance_block_ends_at_the_next_heading_bold_label_or_html_block() {
    let fixture = Declaration::new("okp-gate-acceptance-bounds");
    // Review bots append their own bulleted summaries to the body. Those bullets
    // are not acceptance items and must not be reported as unresolved ones.
    let body = "## Operator acceptance\n\n\
        - [x] Packaged build verified on GNOME\n\n\
        <h3>Review summary</h3>\n\n\
        - Holds native resize until the toplevel acknowledges\n\
        - Adds fullscreen geometry diagnostics\n\n\
        **What the bot did**\n\n\
        - Ran the focused regression harness\n";

    let output = fixture.check("Fix Linux fullscreen screenshot surface restore", body);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn a_completed_operator_acceptance_block_is_not_blocked() {
    let fixture = Declaration::new("okp-gate-acceptance-done");
    let body = "## What this changes\n\nAdds dual-display handling.\n\n\
        ## Operator acceptance\n\n\
        - [x] Packaged build passes GNOME/Wayland dual-display QA\n\n\
        ## Notes\n\n\
        - [ ] follow-up idea, outside the acceptance block\n";

    let output = fixture.check("Handle dual displays on Wayland", body);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn an_acceptance_block_kept_inside_an_html_comment_does_not_block() {
    let fixture = Declaration::new("okp-gate-template");
    let template = fs::read_to_string(repo_root().join(".github/pull_request_template.md"))
        .expect("pull request template should exist");

    let output = fixture.check("Fix the seek readout", &template);

    assert_eq!(
        output.status.code(),
        Some(0),
        "the repository's own template must not block every pull request: {}",
        stderr_of(&output)
    );
}

#[test]
fn quoting_the_gate_inside_a_fenced_block_does_not_block() {
    let fixture = Declaration::new("okp-gate-quoted");
    let body = "## What this changes\n\nDocuments the gate.\n\n\
        ```\n<!-- maestro:wip -->\n## Operator acceptance\n- [ ] example\n```\n";

    let output = fixture.check("Document the merge gate", body);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn an_empty_body_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-empty-body");

    let output = fixture.check("Fix the seek readout", "<!-- nothing but a comment -->\n");

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_of(&output).contains("no description"));
}

#[test]
fn unfinished_code_markers_block_the_merge_unless_they_name_an_issue() {
    let fixture = Declaration::new("okp-gate-code-markers");
    let source = fixture.root.path().join("crate/src/lib.rs");
    fs::create_dir_all(source.parent().expect("parent")).expect("fixture tree");

    fs::write(&source, "pub fn seek() -> u32 {\n    todo!()\n}\n").expect("write");
    let blocked = fixture.check("Add seeking", FINISHED_BODY);
    assert_eq!(blocked.status.code(), Some(1));
    assert!(stderr_of(&blocked).contains("Unfinished-code marker"));

    fs::write(
        &source,
        "// TODO: make this fast\npub fn seek() -> u32 {\n    3\n}\n",
    )
    .expect("write");
    let bare_marker = fixture.check("Add seeking", FINISHED_BODY);
    assert_eq!(bare_marker.status.code(), Some(1));

    fs::write(
        &source,
        "// TODO(#1234): make this fast\npub fn seek() -> u32 {\n    3\n}\n",
    )
    .expect("write");
    let tracked = fixture.check("Add seeking", FINISHED_BODY);
    assert_eq!(tracked.status.code(), Some(0), "{}", stderr_of(&tracked));
}

#[test]
fn an_unfinished_marker_in_shipped_xaml_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-xaml-marker");
    let markup = fixture.root.path().join("Themes/Typography.xaml");
    fs::create_dir_all(markup.parent().expect("parent")).expect("fixture tree");

    fs::write(&markup, "<!-- Timecode 12 (tabular fidelity TODO) -->\n").expect("write");
    let blocked = fixture.check("Add the timecode style", FINISHED_BODY);
    assert_eq!(blocked.status.code(), Some(1), "{}", stderr_of(&blocked));
    assert!(stderr_of(&blocked).contains("Unfinished-code marker"));

    fs::write(
        &markup,
        "<!-- Timecode 12 (tabular fidelity TODO(#638)) -->\n",
    )
    .expect("write");
    let tracked = fixture.check("Add the timecode style", FINISHED_BODY);
    assert_eq!(tracked.status.code(), Some(0), "{}", stderr_of(&tracked));
}

#[test]
fn a_marker_word_inside_a_string_literal_is_not_unfinished_code() {
    let fixture = Declaration::new("okp-gate-marker-literal");
    let source = fixture.root.path().join("crate/src/lib.rs");
    fs::create_dir_all(source.parent().expect("parent")).expect("fixture tree");
    fs::write(
        &source,
        "pub const LABEL: &str = \"TODO\";\npub fn parse(marker: &str) -> bool {\n    marker == \"FIXME\"\n}\n",
    )
    .expect("write");

    let output = fixture.check("Parse marker labels", FINISHED_BODY);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

// ---------------------------------------------------------------------------
// Source-text-only test detection: scripts/check-source-grep-tests.py
// ---------------------------------------------------------------------------

/// A test lifted verbatim from `okp-linux-gtk`: every assertion only checks that
/// the crate's own source text contains a string, so it passes against a broken
/// implementation.
const SOURCE_GREP_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = include_str!("main.rs");
    let renderer = source
        .find("configure_linux_renderer_environment();")
        .expect("main should configure rendering");
    let gtk = source
        .find("VelopackApp::build()")
        .expect("main should initialize Velopack and GTK");

    assert!(renderer < gtk);
}
"#;

/// The behavioural shape of the same intent: drive the code and assert on what
/// it produced.
const BEHAVIOURAL_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let recorder = StartupRecorder::default();
    run_startup(&recorder);

    assert_eq!(recorder.steps(), ["renderer-environment", "gtk-init"]);
}
"#;

/// A fixture-loading test: `include_str!` reads test data, not the crate source,
/// and the assertions are about parsed output.
const FIXTURE_TEST: &str = r#"
#[test]
fn parses_a_three_cue_subtitle_file() {
    let sample = include_str!("../fixtures/three-cues.srt");

    let cues = parse_srt(sample).expect("sample should parse");

    assert_eq!(cues.len(), 3);
    assert_eq!(cues[0].start_ms, 1_000);
}
"#;

/// A behavioural test whose only assertion hands the fixture text straight to
/// production code. The source binding is mentioned, but it is parsed, not
/// inspected, so this is behaviour and must not be flagged.
const INLINE_FIXTURE_TEST: &str = r#"
#[test]
fn parses_a_three_cue_subtitle_file() {
    let sample = include_str!("../fixtures/three-cues.srt");

    assert_eq!(parse_srt(sample).expect("sample should parse").len(), 3);
}
"#;

/// A whole-text comparison. The binding is not sliced and no method is called
/// on it, but the assertion is still about the text and nothing else.
const WHOLE_TEXT_COMPARISON_TEST: &str = r#"
#[test]
fn main_matches_the_expected_implementation() {
    let source = include_str!("main.rs");

    assert_eq!(source, "fn main() { configure(); }");
}
"#;

struct Workspace {
    root: TempDir,
}

impl Workspace {
    fn new(name: &str) -> Self {
        let root = unique_temp_dir(name);
        fs::create_dir_all(root.path().join("rust/crates/demo/src")).expect("fixture tree");
        fs::create_dir_all(root.path().join(".github")).expect("fixture tree");
        Self { root }
    }

    fn write_tests(&self, contents: &str) {
        fs::write(
            self.root.path().join("rust/crates/demo/src/tests.rs"),
            contents,
        )
        .expect("fixture test file should be written");
    }

    fn write_allowlist(&self, contents: &str) {
        fs::write(
            self.root
                .path()
                .join(".github/source-grep-test-allowlist.txt"),
            contents,
        )
        .expect("fixture allowlist should be written");
    }

    fn check(&self) -> Output {
        Command::new("python3")
            .arg(repo_root().join("scripts/check-source-grep-tests.py"))
            .arg("--root")
            .arg(self.root.path())
            .output()
            .expect("source grep check should run")
    }
}

#[test]
fn a_source_text_only_test_is_rejected_when_it_is_not_allowlisted() {
    let workspace = Workspace::new("okp-gate-source-grep");
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("renderer_environment_is_selected_before_gtk_initialization"));
    assert!(stdout.contains("would pass against a broken implementation"));
}

#[test]
fn the_same_test_passes_once_it_is_grandfathered() {
    let workspace = Workspace::new("okp-gate-source-grep-allowlisted");
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_behavioural_test_is_never_flagged() {
    let workspace = Workspace::new("okp-gate-behavioural");
    workspace.write_tests(BEHAVIOURAL_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("source-text-only tests found: 0"));
}

#[test]
fn a_test_that_loads_fixture_data_and_asserts_on_parsed_output_is_never_flagged() {
    let workspace = Workspace::new("okp-gate-fixture-data");
    workspace.write_tests(FIXTURE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn comparing_the_whole_source_text_is_still_a_source_grep() {
    let workspace = Workspace::new("okp-gate-whole-text");
    workspace.write_tests(WHOLE_TEXT_COMPARISON_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("main_matches_the_expected_implementation")
    );
}

#[test]
fn a_fixture_parsed_inside_the_assertion_itself_is_never_flagged() {
    let workspace = Workspace::new("okp-gate-inline-fixture");
    workspace.write_tests(INLINE_FIXTURE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn an_allowlist_entry_that_no_longer_matches_must_be_deleted() {
    let workspace = Workspace::new("okp-gate-stale-allowlist");
    workspace.write_tests(BEHAVIOURAL_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("may only shrink"));
}

/// A second source-text-only test, so a fixture can hold two offenders at once.
const SECOND_SOURCE_GREP_TEST: &str = r#"
#[test]
fn file_association_launches_present_before_media_delivery() {
    let desktop = include_str!("../resources/ok-player.desktop");

    assert!(desktop.contains("MimeType=video/mp4"));
}
"#;

#[test]
fn swapping_a_fixed_test_for_a_new_offender_is_rejected_even_though_the_count_holds() {
    let workspace = GitWorkspace::new("okp-gate-allowlist-swap");
    // Base: one offender, grandfathered.
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );
    let base = workspace.commit("base");

    // Head: that test was fixed, and a brand new source grep was grandfathered
    // in its place. The entry count is unchanged.
    workspace.write_tests(&format!("{BEHAVIOURAL_TEST}{SECOND_SOURCE_GREP_TEST}"));
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::file_association_launches_present_before_media_delivery\n",
    );

    let output = workspace.check(&base);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("may only shrink"));
    assert!(stdout.contains("file_association_launches_present_before_media_delivery"));
}

#[test]
fn moving_a_grandfathered_test_to_another_file_is_accepted() {
    let workspace = GitWorkspace::new("okp-gate-allowlist-move");
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );
    let base = workspace.commit("base");

    workspace.write_tests("");
    workspace.write_moved_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/startup_tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check(&base);

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A workspace with git history, so the allowlist can be compared with its base.
struct GitWorkspace {
    inner: Workspace,
}

impl GitWorkspace {
    fn new(name: &str) -> Self {
        let inner = Workspace::new(name);
        let root = inner.root.path();
        run_git(root, &["init", "--quiet"]);
        run_git(root, &["config", "user.name", "OK Player Tests"]);
        run_git(root, &["config", "user.email", "tests@example.invalid"]);
        Self { inner }
    }

    fn write_tests(&self, contents: &str) {
        self.inner.write_tests(contents);
    }

    fn write_moved_tests(&self, contents: &str) {
        fs::write(
            self.inner
                .root
                .path()
                .join("rust/crates/demo/src/startup_tests.rs"),
            contents,
        )
        .expect("moved fixture test file should be written");
    }

    fn write_allowlist(&self, contents: &str) {
        self.inner.write_allowlist(contents);
    }

    fn commit(&self, message: &str) -> String {
        let root = self.inner.root.path();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "--quiet", "-m", message]);
        String::from_utf8(run_git(root, &["rev-parse", "HEAD"]).stdout)
            .expect("git SHA should be UTF-8")
            .trim()
            .to_owned()
    }

    fn check(&self, base: &str) -> Output {
        Command::new("python3")
            .arg(repo_root().join("scripts/check-source-grep-tests.py"))
            .arg("--root")
            .arg(self.inner.root.path())
            .arg("--base-ref")
            .arg(base)
            .output()
            .expect("source grep check should run")
    }
}

fn run_git(root: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git fixture command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn the_repository_allowlist_covers_exactly_the_tests_that_are_still_source_greps() {
    let output = Command::new("python3")
        .arg(repo_root().join("scripts/check-source-grep-tests.py"))
        .arg("--root")
        .arg(repo_root())
        .output()
        .expect("source grep check should run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "the workspace must stay clean of unlisted source-text-only tests:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
