//! Runs the shared `ok-player://` v1 fixture (`tests/fixtures/ok_player_uri/cases.json`)
//! through the real pipeline: interpret, evaluate with a fake filesystem, plan, and the
//! launch shape. The same file is meant to drive the later Windows port.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use okp_core::ok_player_uri::{
    self, Action, FsKind, FsProbe, LaunchShape, PathFlavor, Plan, PlaybackSummary, QueueMode,
    Reason, Request, StdFsProbe, Target,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    #[allow(dead_code)]
    about: String,
    cases: Vec<Case>,
    plan_cases: Vec<PlanCase>,
    launch_cases: Vec<LaunchCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    input: String,
    #[serde(default)]
    flavor: Flavor,
    #[serde(default)]
    fs: Option<Kind>,
    #[serde(default)]
    probed: Option<String>,
    #[serde(default)]
    fill_to_length: Option<usize>,
    expect: Outcome,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
enum Flavor {
    #[default]
    Posix,
    Windows,
}

impl From<Flavor> for PathFlavor {
    fn from(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Posix => PathFlavor::Posix,
            Flavor::Windows => PathFlavor::Windows,
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Kind {
    File,
    Dir,
    Special,
    Missing,
    Unavailable,
    TimedOut,
}

impl From<Kind> for FsKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::File => FsKind::RegularFile,
            Kind::Dir => FsKind::Directory,
            Kind::Special => FsKind::Special,
            Kind::Missing => FsKind::Missing,
            Kind::Unavailable => FsKind::Unavailable,
            Kind::TimedOut => FsKind::TimedOut,
        }
    }
}

/// The observable result of one link, in the fixture's own shape.
#[derive(Deserialize, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct Outcome {
    result: String,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    path_hex: Option<String>,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    start_dropped: bool,
    #[serde(default)]
    ignored_keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanCase {
    name: String,
    input: String,
    #[serde(default)]
    fs: Option<Kind>,
    current: Option<TargetSpec>,
    plan: PlanSpec,
    presents: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct TargetSpec {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum PlanSpec {
    Open {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        start: Option<f64>,
    },
    Queue {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        mode: String,
        #[serde(default)]
        start_ignored: bool,
    },
    Seek {
        start: f64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchCase {
    name: String,
    args: Vec<String>,
    expect: LaunchOutcome,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct LaunchOutcome {
    result: String,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// A filesystem that answers one kind for every path and records what it was asked.
struct FakeFs {
    kind: Option<FsKind>,
    asked: RefCell<Vec<PathBuf>>,
}

impl FakeFs {
    fn new(kind: Option<Kind>) -> Self {
        Self {
            kind: kind.map(FsKind::from),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl FsProbe for FakeFs {
    fn probe(&self, path: &Path) -> FsKind {
        self.asked.borrow_mut().push(path.to_path_buf());
        self.kind.unwrap_or(FsKind::Unavailable)
    }
}

fn fixture() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ok_player_uri/cases.json");
    serde_json::from_str(&fs::read_to_string(path).expect("read the link fixture"))
        .expect("parse the link fixture")
}

fn path_text(path: &OsStr) -> (Option<String>, Option<String>) {
    match path.to_str() {
        Some(text) => (Some(text.to_owned()), None),
        None => (None, Some(hex(path.as_encoded_bytes()))),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn target_spec(target: &Target) -> TargetSpec {
    match target {
        Target::Url(url) => TargetSpec {
            url: Some(url.clone()),
            path: None,
        },
        Target::Path(path) => TargetSpec {
            url: None,
            path: path_text(path).0,
        },
    }
}

fn target_from_spec(spec: &TargetSpec) -> Target {
    match (&spec.url, &spec.path) {
        (Some(url), None) => Target::Url(url.clone()),
        (None, Some(path)) => Target::Path(path.into()),
        _ => panic!("a target names exactly one of url or path: {spec:?}"),
    }
}

/// Run one link through interpret and evaluate, as a shell would.
fn outcome(input: &str, probe: &FakeFs, flavor: PathFlavor) -> Outcome {
    let reject = |action: Option<Action>, reason: Reason| Outcome {
        result: "reject".to_owned(),
        action: action.map(|action| action.as_str().to_owned()),
        reason: Some(reason.as_str().to_owned()),
        ..Outcome::default()
    };
    let request = match ok_player_uri::interpret(input) {
        None => {
            return Outcome {
                result: "not_ours".to_owned(),
                ..Outcome::default()
            };
        }
        Some(Request::Unsupported { action }) => {
            return Outcome {
                result: "unsupported".to_owned(),
                action: Some(action),
                ..Outcome::default()
            };
        }
        Some(Request::Malformed { action, reason }) => return reject(action, reason),
        Some(Request::Action(request)) => request,
    };
    let validated = match ok_player_uri::evaluate(&request, probe, flavor) {
        Ok(validated) => validated,
        Err(reason) => return reject(Some(request.action), reason),
    };
    let (url, (path, path_hex)) = match validated.target() {
        Target::Url(url) => (Some(url.clone()), (None, None)),
        Target::Path(path) => (None, path_text(path)),
    };
    Outcome {
        result: "accept".to_owned(),
        action: Some(validated.action().as_str().to_owned()),
        reason: None,
        url,
        path,
        path_hex,
        start: validated.start(),
        start_dropped: validated.start_dropped(),
        ignored_keys: validated.ignored_keys().to_vec(),
    }
}

fn fill(text: &mut Option<String>, run: &str) {
    if let Some(text) = text {
        *text = text.replace("{fill}", run);
    }
}

#[test]
fn every_link_case_yields_its_documented_outcome() {
    let fixture = fixture();
    let mut failures = Vec::new();
    let mut names = BTreeSet::new();
    for mut case in fixture.cases {
        assert!(
            names.insert(case.name.clone()),
            "duplicate case {}",
            case.name
        );
        if let Some(length) = case.fill_to_length {
            let run = "a".repeat(length + "{fill}".len() - case.input.len());
            case.input = case.input.replace("{fill}", &run);
            assert_eq!(case.input.len(), length, "{}", case.name);
            fill(&mut case.expect.url, &run);
            fill(&mut case.expect.path, &run);
        }

        let probe = FakeFs::new(case.fs);
        let actual = outcome(&case.input, &probe, case.flavor.into());
        if actual != case.expect {
            failures.push(format!(
                "{}\n    input:    {}\n    expected: {:?}\n    actual:   {actual:?}",
                case.name, case.input, case.expect
            ));
        }

        // The filesystem is asked only after every syntactic rule passed, at most once, and
        // about exactly the path that would play.
        let asked = probe.asked.into_inner();
        match (case.fs.is_some(), asked.as_slice()) {
            (false, []) => {}
            (false, _) => failures.push(format!(
                "{}: a syntactic outcome asked the filesystem about {asked:?}",
                case.name
            )),
            (true, [path]) => {
                let (text, path_hex) = path_text(path.as_os_str());
                let expected_path = case.probed.as_ref().or(case.expect.path.as_ref());
                let matches_expectation = match (&case.expect.path_hex, expected_path) {
                    (Some(expected_hex), _) => path_hex.as_ref() == Some(expected_hex),
                    (None, Some(expected)) => text.as_ref() == Some(expected),
                    (None, None) => true,
                };
                if !matches_expectation {
                    failures.push(format!(
                        "{}: asked the filesystem about {path:?}",
                        case.name
                    ));
                }
            }
            (true, _) => failures.push(format!(
                "{}: expected exactly one filesystem question, got {asked:?}",
                case.name
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_rejection_reason_is_exercised_by_the_fixture() {
    let fixture = fixture();
    let covered = fixture
        .cases
        .iter()
        .filter_map(|case| case.expect.reason.clone())
        .chain(
            fixture
                .launch_cases
                .iter()
                .filter_map(|case| case.expect.reason.clone()),
        )
        .collect::<BTreeSet<_>>();
    let missing = Reason::ALL
        .iter()
        .map(|reason| reason.as_str())
        .filter(|name| !covered.contains(*name))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "no fixture case rejects with {missing:?}"
    );
}

#[test]
fn every_plan_case_yields_its_documented_plan() {
    let fixture = fixture();
    let mut failures = Vec::new();
    for case in fixture.plan_cases {
        let probe = FakeFs::new(case.fs);
        let Some(Request::Action(request)) = ok_player_uri::interpret(&case.input) else {
            panic!("{}: the input must parse to an action", case.name);
        };
        let validated = ok_player_uri::evaluate(&request, &probe, PathFlavor::Posix)
            .unwrap_or_else(|reason| panic!("{}: rejected with {reason:?}", case.name));
        let playback = PlaybackSummary {
            current: case.current.as_ref().map(target_from_spec),
        };
        let plan = ok_player_uri::plan(&validated, &playback);
        let actual = match &plan {
            Plan::Open { target, start } => {
                let TargetSpec { url, path } = target_spec(target);
                PlanSpec::Open {
                    url,
                    path,
                    start: *start,
                }
            }
            Plan::Queue {
                item,
                mode,
                start_ignored,
            } => {
                let TargetSpec { url, path } = target_spec(item);
                PlanSpec::Queue {
                    url,
                    path,
                    mode: match mode {
                        QueueMode::Append => "append",
                        QueueMode::AfterCurrent => "after_current",
                    }
                    .to_owned(),
                    start_ignored: *start_ignored,
                }
            }
            Plan::Seek { start } => PlanSpec::Seek { start: *start },
        };
        if actual != case.plan {
            failures.push(format!(
                "{}: expected {:?}, got {actual:?}",
                case.name, case.plan
            ));
        }
        // Raising follows the action and the playback state alone, so the plan of an
        // accepted request raises exactly when a rejected one with that action would.
        let presents_for_action =
            ok_player_uri::presents_window(Some(validated.action()), &playback);
        if plan.presents() != case.presents || presents_for_action != case.presents {
            failures.push(format!(
                "{}: presents {} / for action {}, expected {}",
                case.name,
                plan.presents(),
                presents_for_action,
                case.presents
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_launch_case_yields_its_documented_shape() {
    for case in fixture().launch_cases {
        let actual = match ok_player_uri::launch_shape(&case.args) {
            LaunchShape::NoLink => LaunchOutcome {
                result: "no_link".to_owned(),
                link: None,
                reason: None,
            },
            LaunchShape::Link(link) => LaunchOutcome {
                result: "link".to_owned(),
                link: Some(link),
                reason: None,
            },
            LaunchShape::Rejected(reason) => LaunchOutcome {
                result: "reject".to_owned(),
                link: None,
                reason: Some(reason.as_str().to_owned()),
            },
        };
        assert_eq!(actual, case.expect, "{}", case.name);
    }
}

/// The real probe against a real directory: the policy plays the original (symlink) path,
/// not a canonical one, and refuses a FIFO and a directory.
#[cfg(unix)]
#[test]
fn the_real_probe_drives_the_policy_on_a_real_filesystem() {
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().expect("temp dir");
    let clip = dir.path().join("clip one.mkv");
    fs::write(&clip, b"x").expect("write clip");
    let alias = dir.path().join("alias.mkv");
    std::os::unix::fs::symlink(&clip, &alias).expect("symlink");
    let fifo = dir.path().join("pipe.mkv");
    let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).expect("fifo path");
    // SAFETY: `fifo_c` is a valid NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);

    let probe = StdFsProbe::default();
    let judge = |path: &Path| {
        let link = format!(
            "ok-player://open{}",
            encode_path(path.as_os_str().as_bytes())
        );
        let Some(Request::Action(request)) = ok_player_uri::interpret(&link) else {
            panic!("{link} must parse");
        };
        ok_player_uri::evaluate(&request, &probe, PathFlavor::Posix)
            .map(|validated| validated.target().clone())
    };

    assert_eq!(judge(&clip), Ok(Target::Path(clip.clone().into())));
    assert_eq!(judge(&alias), Ok(Target::Path(alias.clone().into())));
    assert_eq!(judge(dir.path()), Err(Reason::Directory));
    assert_eq!(judge(&fifo), Err(Reason::NotRegularFile));
    assert_eq!(judge(&dir.path().join("gone.mkv")), Err(Reason::NotFound));
}

#[cfg(unix)]
fn encode_path(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
