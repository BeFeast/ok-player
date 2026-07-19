#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;
use tempfile::TempDir;

#[test]
fn accepts_a_pull_request_with_a_reviewable_file_change() {
    let fixture = GitFixture::new("okp-pr-reviewable-change");
    let base = fixture.commit_file("README.md", "base\n", "base");
    let head = fixture.commit_file(
        "docs/qa-records/2026-07-19-issue-438.md",
        "# QA record\n",
        "record evidence",
    );

    let output = fixture.check(&base, &head);

    assert!(
        output.status.success(),
        "changed pull request should pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("reviewable file changes"));
}

#[test]
fn rejects_an_empty_traceability_commit() {
    let fixture = GitFixture::new("okp-pr-reviewable-empty");
    let base = fixture.commit_file("README.md", "base\n", "base");
    let head = fixture.commit_empty("empty traceability commit");

    let output = fixture.check(&base, &head);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no file changes"));
    assert!(stderr.contains("docs/qa-records/YYYY-MM-DD-issue-NNN.md"));
}

#[test]
fn rejects_a_branch_that_reverts_all_of_its_changes() {
    let fixture = GitFixture::new("okp-pr-reviewable-reverted");
    let base = fixture.commit_file("README.md", "base\n", "base");
    fixture.commit_file("README.md", "changed\n", "change");
    let head = fixture.commit_file("README.md", "base\n", "revert");

    let output = fixture.check(&base, &head);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no file changes"));
}

struct GitFixture {
    root: TempDir,
    script: PathBuf,
}

impl GitFixture {
    fn new(name: &str) -> Self {
        let root = unique_temp_dir(name);
        run_git(root.path(), &["init", "--quiet"]);
        run_git(root.path(), &["config", "user.name", "OK Player Tests"]);
        run_git(
            root.path(),
            &["config", "user.email", "tests@example.invalid"],
        );
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("scripts/check-pr-reviewable-diff.sh");
        Self { root, script }
    }

    fn commit_file(&self, path: &str, contents: &str, message: &str) -> String {
        let path = self.root.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(&path, contents).expect("fixture file should be written");
        run_git(self.root.path(), &["add", "."]);
        run_git(self.root.path(), &["commit", "--quiet", "-m", message]);
        self.head()
    }

    fn commit_empty(&self, message: &str) -> String {
        run_git(
            self.root.path(),
            &["commit", "--quiet", "--allow-empty", "-m", message],
        );
        self.head()
    }

    fn head(&self) -> String {
        let output = run_git(self.root.path(), &["rev-parse", "HEAD"]);
        String::from_utf8(output.stdout)
            .expect("git SHA should be UTF-8")
            .trim()
            .to_owned()
    }

    fn check(&self, base: &str, head: &str) -> Output {
        Command::new("bash")
            .arg(&self.script)
            .arg(base)
            .arg(head)
            .current_dir(self.root.path())
            .output()
            .expect("reviewable diff check should run")
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
