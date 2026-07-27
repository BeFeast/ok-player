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
fn an_acceptance_hold_written_as_a_paragraph_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance-paragraph");
    let body = "## Summary\n\nReveals saved screenshots.\n\n\
        ## Operator acceptance required\n\n\
        Keep this out of main until an operator verifies the reveal on GNOME.\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("states a hold in prose"));
    assert!(stderr.contains("verifies the reveal on GNOME"));
}

#[test]
fn prose_beside_a_ticked_box_still_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance-mixed");
    let body = "## Operator acceptance\n\n\
        - [x] Verified on GNOME\n\n\
        Also verify the packaged build on Windows before merge.\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("states a hold in prose"));
    assert!(stderr.contains("packaged build on Windows"));
}

#[test]
fn a_block_terminator_cannot_hide_the_unticked_boxes_below_it() {
    let fixture = Declaration::new("okp-gate-acceptance-terminator");
    // One ticked box followed by any block terminator used to end the block, so
    // everything after it - including the real hold - was never examined. The
    // bold-label shape is the one a human would write without meaning to cheat.
    for terminator in ["**Still pending**", "<div>", "---", "__Outstanding__"] {
        let body = format!(
            "## Operator acceptance\n\n- [x] smoke run\n\n{terminator}\n\n\
             - [ ] dual-display QA on a packaged build\n"
        );

        let output = fixture.check("Reveal saved Linux screenshots", &body);

        assert_eq!(
            output.status.code(),
            Some(1),
            "terminator must not hide the hold: {terminator}"
        );
        let stderr = stderr_of(&output);
        assert!(stderr.contains("Operator acceptance is not complete"));
        assert!(stderr.contains("dual-display QA on a packaged build"));
    }
}

#[test]
fn a_weakly_opened_acceptance_block_does_not_swallow_a_later_list() {
    let fixture = Declaration::new("okp-gate-acceptance-weak-open");
    // The section scan must not reach past a terminator at least as strong as
    // the label that opened the block, or every follow-up list in the body
    // becomes an acceptance item.
    let body = "**Operator acceptance**\n\n\
        - [x] Verified on GNOME\n\n\
        **Follow-ups**\n\n\
        - [ ] a later idea, not a hold\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn an_unbalanced_code_fence_does_not_disable_the_body_rules() {
    let fixture = Declaration::new("okp-gate-odd-fence");
    // An unterminated fence used to blank every line after it, which switched
    // off the marker and acceptance rules for the rest of the body. Third
    // parties edit pull request bodies here, so an odd fence count is reachable
    // without the author doing anything.
    let body = "## What this changes\n\nRestores the surface.\n\n\
        ```\nan unterminated example\n\n\
        <!-- maestro:wip -->\n\n\
        ## Operator acceptance\n\n\
        - [ ] dual-display QA on a packaged build\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("WIP marker"));
    assert!(stderr.contains("dual-display QA on a packaged build"));
}

#[test]
fn an_acceptance_block_with_nothing_in_it_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance-empty-block");
    // The backstop behind every other acceptance rule: whatever shape a block
    // takes, if the gate can find no items in it, the block states a condition
    // that nothing can record as performed.
    let body = "## Summary\n\nReveals saved screenshots.\n\n\
        ## Operator acceptance\n\n\
        ## Notes\n\nNothing else.\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("Operator acceptance block is empty"));
}

#[test]
fn an_acceptance_hold_under_another_heading_still_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-acceptance-headings");
    // "Operator acceptance" is what the template asks for. A hold does not stop
    // being a hold because it was titled differently - #621 wrote "Live
    // acceptance hold" - so the heading match covers the near neighbours.
    for heading in [
        "## Live acceptance hold",
        "## Acceptance criteria",
        "## Before merge",
        "## Operator sign-off",
    ] {
        let body = format!(
            "## Summary\n\nReveals saved screenshots.\n\n{heading}\n\n- [ ] dual-display QA\n"
        );

        let output = fixture.check("Reveal saved Linux screenshots", &body);

        assert_eq!(
            output.status.code(),
            Some(1),
            "heading should open an acceptance block: {heading}"
        );
        assert!(stderr_of(&output).contains("dual-display QA"));
    }
}

#[test]
fn an_unfinished_marker_in_a_shipped_workflow_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-workflow-marker");
    // The tree scan used to cover code suffixes only, so a half-finished CI job
    // - including the workflows this gate itself ships - was invisible to it.
    let workflow = fixture.root.path().join(".github/workflows/release.yml");
    fs::create_dir_all(workflow.parent().expect("parent")).expect("fixture tree");

    fs::write(
        &workflow,
        "jobs:\n  release:\n    # TODO: sign the artifacts\n",
    )
    .expect("write");
    let blocked = fixture.check("Add the release workflow", FINISHED_BODY);
    assert_eq!(blocked.status.code(), Some(1), "{}", stderr_of(&blocked));
    assert!(stderr_of(&blocked).contains("Unfinished-code marker"));

    fs::write(
        &workflow,
        "jobs:\n  release:\n    # TODO(#1234): sign the artifacts\n",
    )
    .expect("write");
    let tracked = fixture.check("Add the release workflow", FINISHED_BODY);
    assert_eq!(tracked.status.code(), Some(0), "{}", stderr_of(&tracked));
}

#[test]
fn a_nested_heading_does_not_end_an_acceptance_section() {
    let fixture = Declaration::new("okp-gate-acceptance-nested");
    // Grouping acceptance checks under sub-headings is the natural way to write
    // a per-platform hold. A section ends at a heading of its own level or
    // above, not at the first heading of any level.
    let body = "## Operator acceptance\n\n\
        ### Linux\n\n\
        - [x] Verified on GNOME\n\n\
        ### Windows\n\n\
        - [ ] Packaged build verified on Windows 11\n\n\
        ## Notes\n\n\
        - [ ] unrelated follow-up, outside the section\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("Packaged build verified on Windows 11"));
    assert!(
        !stderr.contains("unrelated follow-up"),
        "the section must still end at a heading of its own level:\n{stderr}"
    );
}

#[test]
fn a_marker_between_two_lifetimes_is_not_hidden_by_the_literal_mask() {
    let fixture = Declaration::new("okp-gate-lifetime-mask");
    // The character-literal mask used to run from the quote in `'a` to the
    // quote in `'b`, blanking whatever sat between them - including a marker.
    let source = fixture.root.path().join("crate/src/lib.rs");
    fs::create_dir_all(source.parent().expect("parent")).expect("fixture tree");
    fs::write(
        &source,
        "pub fn choose<'a /* TODO */, 'b>(first: &'a str, second: &'b str) -> &'a str {\n    first\n}\n",
    )
    .expect("write");

    let output = fixture.check("Add the chooser", FINISHED_BODY);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("Unfinished-code marker"));
}

#[test]
fn a_character_literal_holding_a_quote_does_not_derail_the_string_mask() {
    let fixture = Declaration::new("okp-gate-char-literal");
    // Narrowing the character-literal rule must not cost the masking it was
    // there for. A `'"'` on the same line as a string literal steals that
    // string's opening quote when characters are not masked, which leaves the
    // literal's own text exposed and reports a marker that is only data.
    let source = fixture.root.path().join("crate/src/lib.rs");
    fs::create_dir_all(source.parent().expect("parent")).expect("fixture tree");
    fs::write(
        &source,
        "pub fn labels() {\n    let separator = '\"'; let label = \"TODO\";\n    let _ = (separator, label);\n}\n",
    )
    .expect("write");

    let output = fixture.check("Name the marker", FINISHED_BODY);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn a_sentence_that_merely_mentions_operator_acceptance_opens_no_block() {
    let fixture = Declaration::new("okp-gate-acceptance-sentence");
    let body = "## Summary\n\nDocumentation only.\n\n\
        Operator acceptance is not required for this documentation-only change.\n";

    let output = fixture.check("Document the merge gate", body);

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
fn a_backtick_fence_does_not_close_on_a_tilde_fence() {
    let fixture = Declaration::new("okp-gate-mismatched-fence");
    // Pairing fences by position alone let a ``` opener close on a later ~~~
    // line, and everything between them - a WIP marker included - was stripped
    // before any rule saw it. A fence closes only on its own delimiter.
    let body = "## What this changes\n\nRestores the surface.\n\n\
        ```\n<!-- maestro:wip -->\n~~~\n\nMore prose.\n";

    let output = fixture.check("Reveal saved Linux screenshots", body);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("WIP marker"));
}

#[test]
fn a_matching_tilde_fence_still_quotes_the_gate() {
    let fixture = Declaration::new("okp-gate-tilde-fence");
    let body = "## What this changes\n\nDocuments the gate.\n\n\
        ~~~\n<!-- maestro:wip -->\n## Operator acceptance\n- [ ] example\n~~~\n";

    let output = fixture.check("Document the merge gate", body);

    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn a_panicking_stub_written_with_a_detached_bang_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-detached-bang");
    // Rust tokenises `todo !()` exactly like `todo!()`, so the stub compiles.
    let source = fixture.root.path().join("crate/src/lib.rs");
    fs::create_dir_all(source.parent().expect("parent")).expect("fixture tree");
    fs::write(&source, "pub fn seek() -> u32 {\n    todo !()\n}\n").expect("write");

    let output = fixture.check("Add seeking", FINISHED_BODY);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("Unfinished-code marker"));
}

#[test]
fn an_unfinished_marker_in_a_shipped_manifest_blocks_the_merge() {
    let fixture = Declaration::new("okp-gate-manifest-marker");
    let manifest = fixture.root.path().join("app.manifest");
    fs::write(
        &manifest,
        "<assembly>\n  <!-- TODO: declare DPI awareness -->\n</assembly>\n",
    )
    .expect("write");

    let output = fixture.check("Ship the manifest", FINISHED_BODY);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("Unfinished-code marker"));
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

/// The laundered shape: the source greps are untouched and one assertion about
/// nothing at all is added beside them. This is the cheapest possible way to
/// make a detector that counts "non-grep assertions" look satisfied.
const LAUNDERED_SOURCE_GREP_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = include_str!("main.rs");
    assert_eq!(2 + 2, 4);
    let renderer = source
        .find("configure_linux_renderer_environment();")
        .expect("main should configure rendering");
    let gtk = source
        .find("VelopackApp::build()")
        .expect("main should initialize Velopack and GTK");

    assert!(renderer < gtk);
}
"#;

/// `include_str!` behind a same-file helper. The test body never names the
/// macro, so a detector that only reads the test body sees no source text.
const HELPER_FUNCTION_SOURCE_TEST: &str = r#"
fn main_source() -> &'static str {
    include_str!("main.rs")
}

#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = main_source();

    assert!(source.contains("configure_linux_renderer_environment();"));
}
"#;

/// A helper that parses a fixture and returns a value is production code, not
/// source text: a test that uses it must stay unflagged.
const HELPER_PARSER_TEST: &str = r#"
fn sample_cues() -> Vec<Cue> {
    parse_srt(include_str!("../fixtures/three-cues.srt")).expect("sample should parse")
}

#[test]
fn parses_a_three_cue_subtitle_file() {
    let cues = sample_cues();

    assert_eq!(cues.len(), 3);
}
"#;

/// Source text bound through a tuple pattern, then asserted on. `let (a, b) =`
/// was not read as a binding at all, so both halves counted as behaviour.
const TUPLE_BINDING_SOURCE_TEST: &str = r#"
#[test]
fn close_request_defers_engine_teardown() {
    let (source, _unused) = (include_str!("keyboard.rs"), 1);

    assert!(source.contains("glib::idle_add_local_once"));
}
"#;

/// Source texts collected into a vector and greped through a `for` loop.
const LOOP_BINDING_SOURCE_TEST: &str = r#"
#[test]
fn every_shell_module_configures_the_renderer() {
    let modules = vec![include_str!("main.rs"), include_str!("window.rs")];

    for source in modules {
        assert!(source.contains("configure_linux_renderer_environment();"));
    }
}
"#;

/// Source texts written straight into the loop's array literal, never bound to
/// a name at all. This is the shape the real workspace uses.
const INLINE_LOOP_SOURCE_TEST: &str = r#"
#[test]
fn portability_smokes_wait_for_the_window_manager() {
    for script in [
        include_str!("../../../scripts/smoke-narrow-width.sh"),
        include_str!("../../../scripts/smoke-compact-mode.sh"),
    ] {
        assert!(script.contains("run-isolated-dbus-session.sh"));
        assert!(script.contains("wait-for-window.sh"));
    }
}
"#;

/// A source grep beside a text search on a constant the detector does not track
/// as source text. Searching a string is not running the code under test, so it
/// must not clear the `include_str!` greps sitting next to it.
const CONST_TEXT_SEARCH_TEST: &str = r#"
#[test]
fn playback_chrome_keeps_the_canonical_redlines() {
    assert!(OKP_STYLESHEET.contains("min-height: 42px;"));
    assert!(!OKP_STYLESHEET.contains("okp-control-separator"));

    let osc_bar = include_str!("osc_bar.rs");
    assert!(osc_bar.contains("pub(crate) const PAD_HORIZONTAL: i32 = 14;"));
}
"#;

/// The same shape, but the non-grep assertion actually calls production code.
/// This is the false positive the text-search rule must not create.
const PRODUCTION_CALL_BESIDE_A_GREP_TEST: &str = r#"
#[test]
fn media_info_command_uses_the_companion_window_entry_point() {
    let source = include_str!("track_popovers.rs");

    assert_eq!(subtitle_style_label("Contrast"), "High contrast");
    assert!(source.contains("dispatch_player_command_action("));
}
"#;

/// The same three binding shapes, each beside a genuine behavioural assertion.
/// None of these is an offender, so whether the binding tracker sees the source
/// text shows up only in the ledger: a grep it cannot see is a grep that stopped
/// existing, and the allowlist entry is then reported as stale.
const TUPLE_BINDING_WITH_BEHAVIOUR: &str = r#"
#[test]
fn close_request_defers_engine_teardown() {
    let (source, _unused) = (include_str!("keyboard.rs"), 1);

    assert_eq!(close_reason_for("quit"), Reason::Quit);
    assert!(source.contains("glib::idle_add_local_once"));
}
"#;

const LOOP_BINDING_WITH_BEHAVIOUR: &str = r#"
#[test]
fn every_shell_module_configures_the_renderer() {
    let modules = vec![include_str!("main.rs"), include_str!("window.rs")];

    assert_eq!(renderer_for("gl"), Renderer::Gl);
    for source in modules {
        assert!(source.contains("configure_linux_renderer_environment();"));
    }
}
"#;

const INLINE_LOOP_WITH_BEHAVIOUR: &str = r#"
#[test]
fn portability_smokes_wait_for_the_window_manager() {
    assert_eq!(session_kind("xvfb"), SessionKind::Isolated);

    for script in [
        include_str!("../../../scripts/smoke-narrow-width.sh"),
        include_str!("../../../scripts/smoke-compact-mode.sh"),
    ] {
        assert!(script.contains("run-isolated-dbus-session.sh"));
    }
}
"#;

/// A wall of source greps with one unrelated fallible setup call beside them.
/// `current_dir().unwrap()` panics on failure, so it reads as an assertion, but
/// it says nothing about the subject under test.
const UNRELATED_UNWRAP_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let working_directory = std::env::current_dir().unwrap();
    let source = include_str!("main.rs");

    assert!(source.contains("configure_linux_renderer_environment();"));
    assert!(source.contains("VelopackApp::build()"));
}
"#;

/// A fixture parsed through receiver syntax. `sample.parse()` reads like a
/// method on the text, but it runs a production `FromStr`, so what comes back
/// is a production value and asserting on it is behaviour.
const RECEIVER_PARSE_TEST: &str = r#"
#[test]
fn parses_the_sample_configuration() {
    let sample = include_str!("../fixtures/config.toml");

    let config: AppConfig = sample.parse().expect("sample should parse");

    assert_eq!(config.mode, Mode::Compact);
}
"#;

/// A test whose only check is a helper call, with no assertion macro at all.
const HELPER_STATEMENT_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = include_str!("main.rs");

    check_source_contains(source, "configure_linux_renderer_environment();");
}
"#;

/// Inspection wrapped in an assertion macro through an assertion helper.
const ASSERT_HELPER_TEST: &str = r#"
#[test]
fn renderer_environment_is_selected_before_gtk_initialization() {
    let source = include_str!("main.rs");

    assert!(assert_source_contains(source, "configure_linux_renderer_environment();"));
}
"#;

/// A fixture test with no assertion macro at all: the fallible production call
/// is the assertion, and it fails the test when the parser cannot handle the
/// input.
const EXPECT_ONLY_FIXTURE_TEST: &str = r#"
#[test]
fn parses_the_sample_playlist() {
    let sample = include_str!("../fixtures/playlist.m3u");

    parse_playlist(sample).expect("the sample playlist should parse");
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
        self.check_with(&[])
    }

    fn check_with(&self, extra: &[&str]) -> Output {
        Command::new("python3")
            .arg(repo_root().join("scripts/check-source-grep-tests.py"))
            .arg("--root")
            .arg(self.root.path())
            .args(extra)
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
fn one_unrelated_assertion_does_not_launder_a_source_grep() {
    let workspace = Workspace::new("okp-gate-laundered");
    workspace.write_tests(LAUNDERED_SOURCE_GREP_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "assert_eq!(2 + 2, 4) proves nothing about an implementation and must not \
         clear a test made of source greps:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("renderer_environment_is_selected_before_gtk_initialization")
    );
}

#[test]
fn laundering_an_allowlisted_test_does_not_turn_its_ledger_entry_stale() {
    let workspace = Workspace::new("okp-gate-ledger-drain");
    // Keying staleness on "is it still an offender" made the ledger drainable:
    // add one junk assertion to a grandfathered test and the check demanded its
    // line be deleted, so the prescribed fix for laundering was to record the
    // laundering as progress. An entry goes when the last grep goes.
    workspace.write_tests(LAUNDERED_SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        !stdout.contains("Stale source-grep allowlist entry"),
        "the ledger must not ask for the line to be deleted:\n{stdout}"
    );
}

#[test]
fn a_partly_fixed_allowlisted_test_keeps_its_ledger_entry() {
    let workspace = Workspace::new("okp-gate-partial-fix");
    // Genuine partial progress, not laundering: a real behavioural assertion
    // was added and the greps are still there. The entry still may not go.
    let partly_fixed = format!(
        "{}\n",
        SOURCE_GREP_TEST.replace(
            "    assert!(renderer < gtk);",
            "    let recorder = StartupRecorder::default();\n    \
             run_startup(&recorder);\n    \
             assert_eq!(recorder.steps().len(), 2);\n    \
             assert!(renderer < gtk);",
        )
    );
    workspace.write_tests(&partly_fixed);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        !stdout.contains("Stale source-grep allowlist entry"),
        "{stdout}"
    );
}

#[test]
fn include_str_reached_through_a_helper_function_is_still_source_text() {
    let workspace = Workspace::new("okp-gate-helper-fn");
    workspace.write_tests(HELPER_FUNCTION_SOURCE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "one level of indirection must not hide a source grep:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("renderer_environment_is_selected_before_gtk_initialization")
    );
}

#[test]
fn a_helper_that_parses_a_fixture_is_production_code_not_source_text() {
    let workspace = Workspace::new("okp-gate-helper-parser");
    workspace.write_tests(HELPER_PARSER_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "resolving helpers must not flag every test that loads a fixture:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn source_text_bound_through_a_tuple_pattern_is_still_source_text() {
    let workspace = Workspace::new("okp-gate-tuple-binding");
    workspace.write_tests(TUPLE_BINDING_SOURCE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("close_request_defers_engine_teardown")
    );
}

#[test]
fn source_text_iterated_in_a_for_loop_is_still_source_text() {
    let workspace = Workspace::new("okp-gate-loop-binding");
    workspace.write_tests(LOOP_BINDING_SOURCE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("every_shell_module_configures_the_renderer")
    );
}

#[test]
fn source_text_iterated_straight_from_an_array_literal_is_still_source_text() {
    let workspace = Workspace::new("okp-gate-inline-loop");
    workspace.write_tests(INLINE_LOOP_SOURCE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("portability_smokes_wait_for_the_window_manager")
    );
}

#[test]
fn searching_a_constant_string_does_not_count_as_running_the_code() {
    let workspace = Workspace::new("okp-gate-const-search");
    workspace.write_tests(CONST_TEXT_SEARCH_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "a grep against a constant is still a grep, whoever owns the string:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("playback_chrome_keeps_the_canonical_redlines")
    );
}

#[test]
fn a_real_production_call_beside_a_grep_still_counts_as_behaviour() {
    let workspace = Workspace::new("okp-gate-call-beside-grep");
    workspace.write_tests(PRODUCTION_CALL_BESIDE_A_GREP_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "narrowing what counts as a call must not flag a test that drives the \
         code and asserts on the result:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Assert that the ledger still recognises `name` as greping source text - the
/// only observable difference the binding tracker makes once a test also has a
/// behavioural assertion, and the difference between keeping a ledger entry and
/// being told to delete it.
fn assert_ledger_entry_survives(fixture: &str, workspace_name: &str, test_name: &str) {
    let workspace = Workspace::new(workspace_name);
    workspace.write_tests(fixture);
    workspace.write_allowlist(&format!("rust/crates/demo/src/tests.rs::{test_name}\n"));

    let output = workspace.check();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(0),
        "a grep the tracker cannot see is a grep it thinks was fixed:\n{stdout}"
    );
    assert!(
        !stdout.contains("Stale source-grep allowlist entry"),
        "the ledger entry must survive while the grep is still there:\n{stdout}"
    );
}

#[test]
fn a_grep_on_a_tuple_bound_name_keeps_its_ledger_entry() {
    assert_ledger_entry_survives(
        TUPLE_BINDING_WITH_BEHAVIOUR,
        "okp-gate-tuple-ledger",
        "close_request_defers_engine_teardown",
    );
}

#[test]
fn a_grep_on_a_loop_bound_name_keeps_its_ledger_entry() {
    assert_ledger_entry_survives(
        LOOP_BINDING_WITH_BEHAVIOUR,
        "okp-gate-loop-ledger",
        "every_shell_module_configures_the_renderer",
    );
}

#[test]
fn a_grep_bound_from_an_array_literal_keeps_its_ledger_entry() {
    assert_ledger_entry_survives(
        INLINE_LOOP_WITH_BEHAVIOUR,
        "okp-gate-inline-loop-ledger",
        "portability_smokes_wait_for_the_window_manager",
    );
}

#[test]
fn an_unrelated_fallible_setup_call_does_not_launder_a_source_grep() {
    let workspace = Workspace::new("okp-gate-unrelated-unwrap");
    workspace.write_tests(UNRELATED_UNWRAP_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(1),
        "setup that never touches the fixture is not evidence:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("renderer_environment_is_selected_before_gtk_initialization")
    );
}

#[test]
fn a_fixture_parsed_through_receiver_syntax_is_never_flagged() {
    let workspace = Workspace::new("okp-gate-receiver-parse");
    workspace.write_tests(RECEIVER_PARSE_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(
        output.status.code(),
        Some(0),
        "`sample.parse()` runs production code; only text manipulation keeps the \
         result inside the source text:\n{}",
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
fn a_test_whose_only_check_is_a_helper_call_is_flagged() {
    let workspace = Workspace::new("okp-gate-helper-statement");
    workspace.write_tests(HELPER_STATEMENT_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("renderer_environment_is_selected_before_gtk_initialization")
    );
}

#[test]
fn an_assertion_helper_does_not_launder_a_source_grep() {
    let workspace = Workspace::new("okp-gate-assert-helper");
    workspace.write_tests(ASSERT_HELPER_TEST);
    workspace.write_allowlist("# empty\n");

    let output = workspace.check();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("renderer_environment_is_selected_before_gtk_initialization")
    );
}

#[test]
fn a_fixture_parsed_by_a_fallible_call_alone_is_never_flagged() {
    let workspace = Workspace::new("okp-gate-expect-only");
    workspace.write_tests(EXPECT_ONLY_FIXTURE_TEST);
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
fn a_new_offender_cannot_ride_in_on_the_name_of_an_entry_that_did_not_move() {
    let workspace = GitWorkspace::new("okp-gate-allowlist-impersonate");
    // The move exception used to match on the bare test name, so a brand new
    // offender in another file could take the name of an entry that is still in
    // the list under its original key, and be waved through as a file move.
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_moved_tests(SECOND_SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n\
         rust/crates/demo/src/startup_tests.rs::file_association_launches_present_before_media_delivery\n",
    );
    let base = workspace.commit("base");

    // The first test never moved and keeps its entry. A new offender in a third
    // file takes its name; one unrelated entry is dropped to hold the count.
    workspace.write_third_file(&SOURCE_GREP_TEST.replace(
        "let source = include_str!(\"main.rs\");",
        "let source = include_str!(\"window.rs\");",
    ));
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n\
         rust/crates/demo/src/window_tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check(&base);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("may only shrink"), "{stdout}");
    assert!(stdout.contains("window_tests.rs"), "{stdout}");
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

    fn write_third_file(&self, contents: &str) {
        fs::write(
            self.inner
                .root
                .path()
                .join("rust/crates/demo/src/window_tests.rs"),
            contents,
        )
        .expect("third fixture test file should be written");
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
        self.check_with(base, &[])
    }

    fn check_with(&self, base: &str, extra: &[&str]) -> Output {
        Command::new("python3")
            .arg(repo_root().join("scripts/check-source-grep-tests.py"))
            .arg("--root")
            .arg(self.inner.root.path())
            .arg("--base-ref")
            .arg(base)
            .args(extra)
            .output()
            .expect("source grep check should run")
    }

    fn delete_allowlist(&self) {
        fs::remove_file(
            self.inner
                .root
                .path()
                .join(".github/source-grep-test-allowlist.txt"),
        )
        .expect("fixture allowlist should be removable");
    }
}

#[test]
fn a_base_revision_that_does_not_resolve_fails_the_growth_check() {
    let workspace = GitWorkspace::new("okp-gate-base-unreachable");
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );
    workspace.commit("base");

    // A shallow checkout, a stale cache, a deleted branch: the base simply is
    // not there. This used to print "::warning" and exit 0, which it did in
    // this pull request's own CI, so the one rule that catches a new offender
    // arriving with its own allowlist entry never ran.
    let output = workspace.check("0123456789abcdef0123456789abcdef01234567");

    assert_eq!(
        output.status.code(),
        Some(1),
        "an unreadable base must fail, not warn:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("Allowlist growth check could not run"),
        "{stdout}"
    );
    assert!(!stdout.contains("::warning"), "{stdout}");
}

#[test]
fn a_missing_base_revision_fails_when_the_growth_check_is_required() {
    let workspace = GitWorkspace::new("okp-gate-base-missing");
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );
    workspace.commit("base");

    // An empty BASE_REF on a pull request run used to skip the growth check
    // silently and without a warning of any kind.
    let output = workspace.check_with("", &["--require-base-ref"]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("No base revision for the allowlist growth check")
    );
}

#[test]
fn a_base_that_predates_the_allowlist_is_reported_as_its_introduction() {
    let workspace = GitWorkspace::new("okp-gate-base-absent");
    // The one case that is neither growth nor a broken checkout: the ledger did
    // not exist yet. It must be distinguished from an unreadable base, and it
    // must say so out loud rather than passing in silence.
    workspace.write_tests(SOURCE_GREP_TEST);
    workspace.write_allowlist("# empty\n");
    workspace.delete_allowlist();
    let base = workspace.commit("before the ledger existed");

    workspace.write_allowlist(
        "rust/crates/demo/src/tests.rs::renderer_environment_is_selected_before_gtk_initialization\n",
    );

    let output = workspace.check_with(&base, &["--require-base-ref"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("Allowlist introduced"), "{stdout}");
    assert!(!stdout.contains("could not run"), "{stdout}");
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
