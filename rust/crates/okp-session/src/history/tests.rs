//! The history rules driven through a session over a scripted engine: what the
//! session forwards on open, what lands in the history file and when, and what
//! the history queries and events report.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use okp_core::history::{FileEntry, History};
use okp_core::player::{PlayerEvent, PropertyChange};
use okp_mpv::{EndFileReason, EntryEvent, MpvEvent};
use okp_test_fixtures::unique_temp_dir;

use super::*;
use crate::engine::{ChangeListener, Observation};
use crate::machine::SessionMachine;
use crate::protocol::{DispatchResult, Route, SeekMode, SeekPrecision, SourceKind};
use crate::session::Session;

/// An engine the test scripts: it records what it is told and reports what the
/// test publishes.
#[derive(Default)]
struct Script {
    forwarded: Mutex<Vec<Command>>,
    observation: Mutex<Observation>,
    events: Mutex<Vec<EntryEvent>>,
    listener: Mutex<Option<ChangeListener>>,
}

struct ScriptedEngine(Arc<Script>);

impl Engine for ScriptedEngine {
    fn forward(&self, command: &Command, _request_id: u64) -> Result<Option<i64>, String> {
        lock(&self.0.forwarded).push(command.clone());
        Ok(None)
    }

    fn observe(&self) -> Observation {
        lock(&self.0.observation).clone()
    }

    fn take_events(&self) -> Vec<EntryEvent> {
        std::mem::take(&mut *lock(&self.0.events))
    }

    fn set_listener(&self, listener: Option<ChangeListener>) {
        *lock(&self.0.listener) = listener;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A session over the scripted engine, with its history in `data_dir`.
struct Harness {
    session: Session,
    script: Arc<Script>,
    data_dir: PathBuf,
    writer: writer::Probe,
}

impl Harness {
    fn new(data_dir: &Path) -> Self {
        Self::with_store(HistoryStore::open(data_dir), data_dir)
    }

    fn with_store(store: HistoryStore, data_dir: &Path) -> Self {
        let script = Arc::new(Script::default());
        let history = SessionHistory::with_store(store);
        let writer = history.probe().expect("the history is kept in a file");
        let session = Session::start(Box::new(ScriptedEngine(Arc::clone(&script))), history);
        Self {
            session,
            script,
            data_dir: data_dir.to_owned(),
            writer,
        }
    }

    /// The entry for `key` in the history file, once every save handed to the
    /// writer has been written.
    fn saved(&self, key: &str) -> Option<FileEntry> {
        self.writer.settle();
        saved(&self.data_dir, key)
    }

    /// The history file's bytes, once every save has been written.
    fn file(&self) -> Vec<u8> {
        self.writer.settle();
        fs::read(history_path(&self.data_dir)).expect("the history file")
    }

    fn dispatch(&self, command: Command) -> DispatchResult {
        self.session.dispatch(&command)
    }

    fn open(&self, location: &str) {
        let reply = self.dispatch(open(location, None));
        assert!(
            matches!(reply, DispatchResult::Accepted { .. }),
            "{reply:?}"
        );
    }

    /// Change what the engine reports and queue its events, then let the
    /// session fold them in (a query refreshes on the calling thread).
    fn publish(&self, change: impl FnOnce(&mut Observation), events: Vec<MpvEvent>) {
        change(&mut lock(&self.script.observation));
        lock(&self.script.events).extend(events.into_iter().map(|event| EntryEvent {
            event,
            playlist_entry_id: None,
        }));
        let _ = self.session.snapshot();
    }

    fn load(&self, duration: f64, tracks: Vec<okp_mpv::Track>) {
        self.publish(
            |observation| {
                observation.playback.duration = Some(duration);
                observation.playback.time_pos = Some(0.0);
                observation.tracks = tracks;
            },
            vec![MpvEvent::FileLoaded {
                video_dimensions: None,
            }],
        );
    }

    fn play_to(&self, position: f64) {
        self.publish(
            |observation| observation.playback.time_pos = Some(position),
            vec![],
        );
    }

    fn forwarded(&self) -> Vec<Command> {
        lock(&self.script.forwarded).clone()
    }

    /// The open the engine was handed last.
    fn last_open(&self) -> Command {
        self.forwarded()
            .into_iter()
            .rev()
            .find(|command| matches!(command, Command::Open { .. }))
            .expect("an open was forwarded")
    }

    fn history_events(&self) -> Vec<HistoryEvent> {
        self.session
            .drain_events()
            .into_iter()
            .filter_map(|event| match event {
                Event::History { event } => Some(event),
                _ => None,
            })
            .collect()
    }

    fn act(&self, action: HistoryAction) -> HistoryActionResult {
        match self.dispatch(Command::History { action }) {
            DispatchResult::History { result } => result,
            other => panic!("a history action answered {other:?}"),
        }
    }

    fn page(&self, search: &str, utc_offset_seconds: i32) -> (bool, Vec<HistorySection>) {
        match self.session.query(&crate::protocol::Query::History {
            query: HistoryQuery::Page {
                search: search.to_owned(),
                utc_offset_seconds,
            },
        }) {
            crate::protocol::QueryResult::History {
                result:
                    HistoryQueryResult::Page {
                        private_session,
                        sections,
                        ..
                    },
            } => (private_session, sections),
            other => panic!("the page query answered {other:?}"),
        }
    }

    /// What the History page says about reading the file: whether it failed,
    /// and where the unreadable file was moved.
    fn read_status(&self) -> (bool, Option<String>) {
        match self.session.query(&crate::protocol::Query::History {
            query: HistoryQuery::Page {
                search: String::new(),
                utc_offset_seconds: 0,
            },
        }) {
            crate::protocol::QueryResult::History {
                result:
                    HistoryQueryResult::Page {
                        read_failed,
                        unreadable_copy,
                        ..
                    },
            } => (read_failed, unreadable_copy),
            other => panic!("the page query answered {other:?}"),
        }
    }
}

fn open(location: &str, resume_from: Option<f64>) -> Command {
    Command::Open {
        source: MediaSource {
            kind: if location.contains("://") {
                SourceKind::Url
            } else {
                SourceKind::Local
            },
            location: location.to_owned(),
        },
        resume_from,
        audio_track: None,
        subtitle_track: None,
    }
}

fn resume_of(command: &Command) -> Option<f64> {
    match command {
        Command::Open { resume_from, .. } => *resume_from,
        other => panic!("{other:?} is not an open"),
    }
}

fn track(kind: okp_mpv::TrackKind, id: i64) -> okp_mpv::Track {
    okp_mpv::Track {
        id,
        kind,
        selected: id == 1 && kind == okp_mpv::TrackKind::Audio,
        external: false,
        external_filename: None,
        default: id == 1,
        title: None,
        lang: None,
        codec: None,
        audio_channels: None,
    }
}

fn two_of_each() -> Vec<okp_mpv::Track> {
    vec![
        track(okp_mpv::TrackKind::Audio, 1),
        track(okp_mpv::TrackKind::Audio, 2),
        track(okp_mpv::TrackKind::Subtitle, 1),
        track(okp_mpv::TrackKind::Subtitle, 2),
    ]
}

fn history_path(dir: &Path) -> PathBuf {
    dir.join(HISTORY_FILE_NAME)
}

fn saved(dir: &Path, key: &str) -> Option<FileEntry> {
    let text = fs::read_to_string(history_path(dir)).ok()?;
    History::load(&text)?.files.remove(key)
}

/// Write a history file holding `entries` of (key, position, duration).
fn seed(dir: &Path, entries: &[(&str, f64, f64)]) {
    let mut history = History::default();
    for (key, position, duration) in entries {
        history.files.insert(
            (*key).to_owned(),
            FileEntry {
                position: *position,
                duration: *duration,
                updated_at_unix: 1_700_000_000,
                ..FileEntry::default()
            },
        );
    }
    fs::create_dir_all(dir).expect("data directory");
    fs::write(
        history_path(dir),
        serde_json::to_vec_pretty(&history).expect("history serialises"),
    )
    .expect("seed history");
}

#[test]
fn reopening_resumes_by_the_core_rules_at_their_boundaries() {
    let root = unique_temp_dir("okp-session-history-resume-rules");
    // 1000 s: the first 5% ends at 50 s and the last 30 s start at 970 s. A
    // 100 s file ends with its last 5% (95 s) instead.
    let cases = [
        ("/media/below-five-percent.mkv", 49.0, 1000.0, None),
        ("/media/at-five-percent.mkv", 50.0, 1000.0, None),
        ("/media/past-five-percent.mkv", 50.5, 1000.0, Some(50.5)),
        ("/media/before-last-30s.mkv", 969.5, 1000.0, Some(969.5)),
        ("/media/at-last-30s.mkv", 970.0, 1000.0, None),
        ("/media/inside-last-30s.mkv", 985.0, 1000.0, None),
        ("/media/short-before-last-5pct.mkv", 94.0, 100.0, Some(94.0)),
        ("/media/short-at-last-5pct.mkv", 95.0, 100.0, None),
    ];
    seed(
        root.path(),
        &cases
            .iter()
            .map(|(key, position, duration, _)| (*key, *position, *duration))
            .collect::<Vec<_>>(),
    );
    let harness = Harness::new(root.path());

    for (key, _, _, expected) in cases {
        harness.open(key);
        assert_eq!(resume_of(&harness.last_open()), expected, "{key}");
    }
    // A position the opener names wins over the remembered one.
    let _ = harness.dispatch(open("/media/past-five-percent.mkv", Some(5.0)));
    assert_eq!(resume_of(&harness.last_open()), Some(5.0));
}

#[test]
fn a_resumed_open_announces_where_it_started_once_loaded() {
    let root = unique_temp_dir("okp-session-history-resumed-event");
    seed(root.path(), &[("/media/film.mkv", 120.0, 600.0)]);
    let harness = Harness::new(root.path());

    harness.open("/media/film.mkv");
    assert_eq!(harness.history_events(), vec![], "nothing before the load");
    harness.load(600.0, vec![]);
    assert_eq!(
        harness.history_events(),
        vec![HistoryEvent::Resumed { position: 120.0 }]
    );

    // An explicit start position is the opener's own choice: no announcement.
    let _ = harness.dispatch(open("/media/film.mkv", Some(30.0)));
    harness.load(600.0, vec![]);
    assert!(
        !harness
            .history_events()
            .iter()
            .any(|event| matches!(event, HistoryEvent::Resumed { .. }))
    );
}

/// Play `key` to 120 s of 600 s with nothing saved yet.
fn playing(root: &Path, key: &str) -> Harness {
    let harness = Harness::new(root);
    harness.open(key);
    harness.load(600.0, vec![]);
    harness.play_to(120.0);
    assert_eq!(
        harness.saved(key),
        None,
        "nothing is saved before the cadence"
    );
    harness
}

#[test]
fn pausing_saves_progress() {
    let root = unique_temp_dir("okp-session-history-pause");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    assert_eq!(
        harness.saved("/media/a.mkv").map(|entry| entry.position),
        Some(120.0)
    );
}

#[test]
fn closing_saves_progress() {
    let root = unique_temp_dir("okp-session-history-close");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::Close {});
    let entry = harness
        .saved("/media/a.mkv")
        .expect("closing saved the entry");
    assert_eq!((entry.position, entry.duration), (120.0, 600.0));
}

#[test]
fn opening_the_next_media_saves_the_progress_of_the_one_it_replaces() {
    let root = unique_temp_dir("okp-session-history-item-change");
    let harness = playing(root.path(), "/media/a.mkv");
    harness.open("/media/b.mkv");
    assert_eq!(
        harness.saved("/media/a.mkv").map(|entry| entry.position),
        Some(120.0)
    );
    assert_eq!(harness.saved("/media/b.mkv"), None, "b has not loaded");
}

#[test]
fn closing_the_session_saves_progress() {
    let root = unique_temp_dir("okp-session-history-quit");
    let harness = playing(root.path(), "/media/a.mkv");
    drop(harness);
    assert_eq!(
        saved(root.path(), "/media/a.mkv").map(|entry| entry.position),
        Some(120.0)
    );
}

#[test]
fn the_engine_quitting_saves_progress() {
    let root = unique_temp_dir("okp-session-history-engine-quit");
    let harness = playing(root.path(), "/media/a.mkv");
    harness.publish(|_| {}, vec![MpvEvent::Shutdown]);
    assert_eq!(
        harness.saved("/media/a.mkv").map(|entry| entry.position),
        Some(120.0)
    );
}

#[test]
fn playing_to_the_end_marks_the_entry_finished() {
    let root = unique_temp_dir("okp-session-history-finished");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let _ = harness.dispatch(Command::SetPaused { paused: false });
    // At the end the engine has unloaded the file and reports no position.
    harness.publish(
        |observation| {
            observation.playback.time_pos = None;
            observation.playback.duration = None;
        },
        vec![MpvEvent::EndFile {
            reason: EndFileReason::Eof,
            path: None,
            diagnostic_messages: vec![],
            last_duration: None,
        }],
    );
    let entry = harness.saved("/media/a.mkv").expect("the entry is kept");
    assert!(entry.finished);
    assert_eq!(entry.position, 0.0);

    // Nothing later moves it back, and reopening starts at the beginning.
    let _ = harness.dispatch(Command::Close {});
    harness.open("/media/a.mkv");
    assert_eq!(resume_of(&harness.last_open()), None);
    assert!(
        harness
            .saved("/media/a.mkv")
            .is_some_and(|entry| entry.finished)
    );
}

/// An engine report with no title and no failed end of file.
fn report(loaded: bool) -> Report<'static> {
    Report {
        loaded,
        end_failed: false,
        title: None,
    }
}

/// The end of file the engine reports when media plays to its end, with the
/// messages it logged on the way.
fn end_of_file(diagnostic_messages: Vec<String>) -> MpvEvent {
    MpvEvent::EndFile {
        reason: EndFileReason::Eof,
        path: None,
        diagnostic_messages,
        last_duration: None,
    }
}

#[test]
fn a_failed_command_before_a_clean_end_of_file_still_finishes_the_entry() {
    let root = unique_temp_dir("okp-session-history-failed-reply");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let _ = harness.dispatch(Command::SetPaused { paused: false });
    // The stream refuses a seek; the engine's reply fails long before the end.
    let DispatchResult::Accepted { request_id } = harness.dispatch(Command::Seek {
        seconds: 300.0,
        mode: SeekMode::Absolute,
        precision: SeekPrecision::Keyframe,
    }) else {
        panic!("the seek was not accepted")
    };
    harness.publish(
        |_| {},
        vec![MpvEvent::CommandReply {
            request_id,
            error: -12,
        }],
    );
    harness.play_to(590.0);
    harness.publish(
        |observation| {
            observation.playback.time_pos = None;
            observation.playback.duration = None;
        },
        vec![end_of_file(vec![])],
    );

    let entry = harness.saved("/media/a.mkv").expect("the entry is kept");
    assert!(entry.finished, "{entry:?}");
    assert_eq!(entry.position, 0.0);
}

#[test]
fn an_end_of_file_after_a_decoder_failure_keeps_the_progress_unfinished() {
    let root = unique_temp_dir("okp-session-history-decoder-failure");
    let harness = playing(root.path(), "/media/a.mkv");
    harness.publish(
        |_| {},
        vec![end_of_file(vec![
            "Failed to initialize a decoder for codec 'hevc'.".to_owned(),
        ])],
    );

    let entry = harness.saved("/media/a.mkv").expect("the progress is kept");
    assert!(!entry.finished, "{entry:?}");
    assert_eq!(entry.position, 120.0);
}

#[test]
fn many_preference_changes_are_written_once_with_the_next_save() {
    let root = unique_temp_dir("okp-session-history-rapid-preferences");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let _ = harness.dispatch(Command::SetPaused { paused: false });
    let before = harness.file();
    let writes = harness.writer.writes();

    // A dragged volume slider and a held subtitle-delay key.
    for step in 1..=30 {
        for command in [
            Command::SetVolume {
                volume: f64::from(step),
            },
            Command::SetSubtitleDelay {
                seconds: f64::from(step) / 10.0,
            },
        ] {
            assert!(matches!(
                harness.dispatch(command),
                DispatchResult::Accepted { .. }
            ));
        }
    }
    assert_eq!(
        harness.file(),
        before,
        "nothing is written while the values change"
    );
    assert_eq!(harness.writer.writes(), writes);

    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let entry = harness.saved("/media/a.mkv").expect("the entry is kept");
    assert_eq!(
        harness.writer.writes(),
        writes + 1,
        "one write for all of it"
    );
    assert_eq!(entry.preferences.volume, Some(30.0));
    assert_eq!(entry.preferences.subtitle_delay, Some(3.0));
}

/// While the disk takes its time over a save, the session keeps answering: the
/// file is written on the writer thread, not under the session lock.
#[cfg(unix)]
#[test]
fn a_slow_disk_never_holds_up_the_session() {
    let root = unique_temp_dir("okp-session-history-slow-disk");
    let harness = playing(root.path(), "/media/a.mkv");
    // The writer's temporary file is a pipe nobody reads yet, so the next
    // write blocks until the test reads it.
    let temporary = root.path().join("history.json.tmp");
    let made = std::process::Command::new("mkfifo")
        .arg(&temporary)
        .status()
        .expect("mkfifo runs");
    assert!(made.success(), "mkfifo failed: {made}");

    let (answered, document) = std::thread::scope(|scope| {
        let (done, finished) = std::sync::mpsc::channel();
        let harness = &harness;
        scope.spawn(move || {
            let _ = harness.dispatch(Command::SetPaused { paused: true });
            let _ = harness.dispatch(Command::SetVolume { volume: 50.0 });
            let _ = harness.session.snapshot();
            let _ = harness.page("", 0);
            let _ = harness.session.drain_events();
            let _ = done.send(());
        });
        let answered = finished.recv_timeout(Duration::from_secs(10)).is_ok();
        // Reading the pipe lets the stuck write through either way.
        let document = fs::read(&temporary).expect("read what the writer wrote");
        (answered, document)
    });
    assert!(answered, "the session waited for the disk");
    assert!(
        String::from_utf8_lossy(&document).contains("/media/a.mkv"),
        "the pause handed its save to the writer"
    );
}

#[test]
fn the_event_thread_wakes_for_a_timed_save() {
    let root = unique_temp_dir("okp-session-history-timed-wake");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    assert_eq!(harness.session.next_wake(), None, "paused, nothing is due");

    let before = Instant::now();
    let _ = harness.dispatch(Command::SetVolume { volume: 40.0 });
    let after = Instant::now();
    let wake = harness
        .session
        .next_wake()
        .expect("the waiting change wakes the event thread");
    assert!(
        wake >= before + PROGRESS_SAVE_INTERVAL && wake <= after + PROGRESS_SAVE_INTERVAL,
        "woken {:?} after the change",
        wake - before
    );
}

#[test]
fn a_failed_save_is_written_again_with_the_next_save() {
    let root = unique_temp_dir("okp-session-history-save-retry");
    let data = root.path().join("data");
    fs::write(&data, b"not a directory").expect("blocking file");
    let harness = Harness::new(&data);
    harness.open("/media/a.mkv");
    harness.load(600.0, vec![]);
    harness.play_to(120.0);
    let _ = harness.dispatch(Command::SetVolume { volume: 40.0 });
    let _ = harness.dispatch(Command::Close {});
    harness.writer.settle();
    harness.publish(|_| {}, vec![]);

    // Nothing changes after the failure; closing the session still writes it.
    fs::remove_file(&data).expect("unblock the data directory");
    drop(harness);
    let entry = saved(&data, "/media/a.mkv").expect("the retried save wrote the entry");
    assert_eq!(entry.position, 120.0);
    assert_eq!(entry.preferences.volume, Some(40.0));
}

#[test]
fn a_preference_change_while_paused_is_saved_after_the_interval() {
    let root = unique_temp_dir("okp-session-history-paused-preference");
    let script = Arc::new(Script::default());
    let engine = ScriptedEngine(Arc::clone(&script));
    let mut history = SessionHistory::open(root.path());
    let writer = history.probe().expect("the history is kept in a file");
    let mut machine = SessionMachine::new();
    let mut queue = Coalescer::new();
    let mut dispatch = |history: &mut SessionHistory, command: Command| {
        let command = history.prepare(&command, &mut queue).into_owned();
        let Route::Player(core) = command.route() else {
            panic!("{command:?} goes to the player")
        };
        let _ = machine.dispatch(&core, |_, _| Ok(()));
        history.dispatched(&command, true, machine.machine(), &engine, &mut queue);
        if matches!(command, Command::Open { .. }) {
            machine.apply_event(PlayerEvent::Loaded {
                duration: Some(600.0),
            });
            history.refresh(
                report(true),
                machine.machine(),
                &engine,
                &mut queue,
                Instant::now(),
            );
        }
    };
    dispatch(&mut history, open("/media/a.mkv", None));
    dispatch(&mut history, Command::SetPaused { paused: true });
    assert_eq!(history.next_save(), None, "nothing waits for a save");
    dispatch(&mut history, Command::SetVolume { volume: 40.0 });
    let due = history
        .next_save()
        .expect("the change waits for a timed save");

    let mut refresh_at = |history: &mut SessionHistory, at: Instant| {
        history.refresh(report(false), machine.machine(), &engine, &mut queue, at);
        writer.settle();
        saved(root.path(), "/media/a.mkv").and_then(|entry| entry.preferences.volume)
    };
    assert_eq!(
        refresh_at(&mut history, due - Duration::from_millis(1)),
        None
    );
    assert_eq!(refresh_at(&mut history, due), Some(40.0));
    assert_eq!(history.next_save(), None);
    assert_eq!(writer.writes(), 1);
}

#[test]
fn an_unreadable_history_file_is_kept_when_the_first_save_writes_a_new_one() {
    let root = unique_temp_dir("okp-session-history-unreadable");
    let data = root.path().join("data");
    fs::create_dir_all(&data).expect("data directory");
    // A history from a newer build, which this one cannot read.
    let original =
        br#"{"version":3,"files":{"/media/keep.mkv":{"position":120.0,"duration":600.0}}}"#;
    fs::write(history_path(&data), original).expect("unreadable history");
    let harness = Harness::new(&data);
    assert_eq!(harness.read_status(), (true, None));

    harness.open("/media/a.mkv");
    harness.load(600.0, vec![]);
    harness.play_to(120.0);
    let _ = harness.dispatch(Command::SetPaused { paused: true });

    assert_eq!(
        harness.saved("/media/a.mkv").map(|entry| entry.position),
        Some(120.0)
    );
    let kept: Vec<PathBuf> = fs::read_dir(&data)
        .expect("data directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("history.json.unreadable-")
            })
        })
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        fs::read(&kept[0]).expect("the kept file"),
        original,
        "the unreadable file survives byte for byte"
    );
    // The page keeps its warning and says where the file went.
    assert_eq!(
        harness.read_status(),
        (true, Some(kept[0].to_string_lossy().into_owned()))
    );
}

#[test]
fn progress_is_saved_every_ten_seconds_while_playing() {
    let root = unique_temp_dir("okp-session-history-cadence");
    let script = Arc::new(Script::default());
    let engine = ScriptedEngine(Arc::clone(&script));
    let mut history = SessionHistory::open(root.path());
    let mut machine = SessionMachine::new();
    let mut queue = Coalescer::new();

    let command = history
        .prepare(&open("/media/a.mkv", None), &mut queue)
        .into_owned();
    let Route::Player(core) = command.route() else {
        panic!("an open goes to the player")
    };
    let _ = machine.dispatch(&core, |_, _| Ok(()));
    history.dispatched(&command, true, machine.machine(), &engine, &mut queue);
    machine.apply_event(PlayerEvent::Loaded {
        duration: Some(600.0),
    });
    let start = Instant::now();
    history.refresh(report(true), machine.machine(), &engine, &mut queue, start);
    let writer = history.probe().expect("the history is kept in a file");

    let mut at = |seconds: f64| {
        machine.apply_event(PlayerEvent::Property(PropertyChange::TimePos(Some(
            100.0 + seconds,
        ))));
        history.refresh(
            report(false),
            machine.machine(),
            &engine,
            &mut queue,
            start + Duration::from_secs_f64(seconds),
        );
        writer.settle();
        saved(root.path(), "/media/a.mkv").map(|entry| entry.position)
    };
    assert_eq!(at(9.9), None);
    assert_eq!(at(10.0), Some(110.0));
    assert_eq!(at(19.0), Some(110.0));
    assert_eq!(at(20.0), Some(120.0));
    assert_eq!(PROGRESS_SAVE_INTERVAL, Duration::from_secs(10));
}

#[test]
fn tracks_subtitle_delay_volume_and_speed_are_restored_on_reopen() {
    let root = unique_temp_dir("okp-session-history-preferences");
    let first = Harness::new(root.path());
    first.open("/media/film.mkv");
    first.load(600.0, two_of_each());
    for command in [
        Command::SelectTrack {
            kind: TrackKind::Audio,
            id: Some(2),
        },
        Command::SelectTrack {
            kind: TrackKind::Subtitle,
            id: None,
        },
        Command::SetSubtitleDelay { seconds: 0.3 },
        Command::SetVolume { volume: 40.0 },
        Command::SetSpeed { speed: 1.5 },
    ] {
        assert!(matches!(
            first.dispatch(command),
            DispatchResult::Accepted { .. }
        ));
    }
    drop(first);

    let second = Harness::new(root.path());
    second.open("/media/film.mkv");
    // Engine-global values go out with the open, before the media plays.
    assert_eq!(
        second.forwarded()[1..],
        [
            Command::SetVolume { volume: 40.0 },
            Command::SetSpeed { speed: 1.5 },
        ]
    );
    second.load(600.0, two_of_each());
    assert_eq!(
        second.forwarded()[3..],
        [
            Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(2),
            },
            Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: None,
            },
            Command::SetSubtitleDelay { seconds: 0.3 },
        ]
    );
}

#[test]
fn a_remembered_track_the_file_lacks_is_left_to_the_engine() {
    let root = unique_temp_dir("okp-session-history-missing-track");
    let first = Harness::new(root.path());
    first.open("/media/film.mkv");
    first.load(600.0, two_of_each());
    let _ = first.dispatch(Command::SelectTrack {
        kind: TrackKind::Audio,
        id: Some(2),
    });
    drop(first);

    let second = Harness::new(root.path());
    second.open("/media/film.mkv");
    second.load(600.0, vec![track(okp_mpv::TrackKind::Audio, 1)]);
    assert_eq!(second.forwarded().len(), 1, "{:?}", second.forwarded());
}

#[test]
fn an_opens_own_track_wins_over_the_remembered_one() {
    let root = unique_temp_dir("okp-session-history-explicit-track");
    let first = Harness::new(root.path());
    first.open("/media/film.mkv");
    first.load(600.0, two_of_each());
    let _ = first.dispatch(Command::SelectTrack {
        kind: TrackKind::Audio,
        id: Some(2),
    });
    drop(first);

    let second = Harness::new(root.path());
    let _ = second.dispatch(Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: "/media/film.mkv".to_owned(),
        },
        resume_from: None,
        audio_track: Some(1),
        subtitle_track: None,
    });
    second.load(600.0, two_of_each());
    let audio: Vec<_> = second
        .forwarded()
        .into_iter()
        .filter_map(|command| match command {
            Command::SelectTrack {
                kind: TrackKind::Audio,
                id,
            } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(
        audio,
        vec![Some(2), Some(1)],
        "the open's choice comes last"
    );
}

#[test]
fn a_private_session_leaves_the_history_file_byte_identical() {
    let root = unique_temp_dir("okp-session-history-private");
    let media = root.path().join("media");
    fs::create_dir_all(&media).expect("media directory");
    let watched = media.join("watched.mkv");
    let fresh = media.join("fresh.mkv");
    for path in [&watched, &fresh] {
        fs::write(path, b"media").expect("media fixture");
    }
    let watched_key = watched.to_string_lossy().into_owned();
    let fresh_key = fresh.to_string_lossy().into_owned();
    let data = root.path().join("data");
    seed(&data, &[(&watched_key, 120.0, 600.0)]);
    let before = fs::read(history_path(&data)).expect("seeded history");

    let harness = Harness::new(&data);
    assert_eq!(
        harness.act(HistoryAction::SetPrivate { enabled: true }),
        HistoryActionResult::Done {}
    );
    assert_eq!(
        harness.history_events(),
        vec![HistoryEvent::PrivateSession { enabled: true }]
    );
    assert_eq!(
        harness.act(HistoryAction::SetPrivate { enabled: true }),
        HistoryActionResult::Unchanged {}
    );

    harness.open(&watched_key);
    assert_eq!(
        resume_of(&harness.last_open()),
        None,
        "nothing remembered is applied"
    );
    harness.load(600.0, two_of_each());
    for command in [
        Command::SetVolume { volume: 30.0 },
        Command::SetSpeed { speed: 2.0 },
        Command::SelectTrack {
            kind: TrackKind::Audio,
            id: Some(2),
        },
        Command::SetSubtitleDelay { seconds: 1.0 },
    ] {
        let _ = harness.dispatch(command);
    }
    harness.play_to(300.0);
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    harness.open(&fresh_key);
    harness.load(900.0, vec![]);
    harness.play_to(400.0);
    harness.publish(
        |_| {},
        vec![MpvEvent::EndFile {
            reason: EndFileReason::Eof,
            path: None,
            diagnostic_messages: vec![],
            last_duration: None,
        }],
    );

    // The history stays readable while nothing is recorded.
    let (private_session, sections) = harness.page("", 0);
    assert!(private_session);
    let keys: Vec<_> = sections
        .iter()
        .flat_map(|section| section.entries.iter())
        .map(|entry| entry.source.location.clone())
        .collect();
    assert_eq!(keys, vec![watched_key.clone()]);
    drop(harness);

    assert_eq!(
        fs::read(history_path(&data)).expect("history file"),
        before,
        "a private session must not touch the history file"
    );
    assert!(!data.join("history.json.tmp").exists());
}

#[test]
fn removing_an_entry_persists_and_stops_recording_the_playing_media() {
    let root = unique_temp_dir("okp-session-history-remove");
    let harness = playing(root.path(), "/media/a.mkv");
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let _ = harness.history_events();
    let source = MediaSource {
        kind: SourceKind::Local,
        location: "/media/a.mkv".to_owned(),
    };

    assert_eq!(
        harness.act(HistoryAction::Remove {
            source: source.clone()
        }),
        HistoryActionResult::Done {}
    );
    assert_eq!(harness.history_events(), vec![HistoryEvent::Changed {}]);
    assert_eq!(harness.saved("/media/a.mkv"), None);

    // The removed media keeps playing without coming back into the history.
    let _ = harness.dispatch(Command::SetPaused { paused: false });
    harness.play_to(200.0);
    let _ = harness.dispatch(Command::SetVolume { volume: 50.0 });
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    assert_eq!(harness.saved("/media/a.mkv"), None);
    assert_eq!(
        harness.act(HistoryAction::Remove { source }),
        HistoryActionResult::Unchanged {}
    );

    // Opening it again records it again.
    harness.open("/media/a.mkv");
    harness.load(600.0, vec![]);
    harness.play_to(90.0);
    let _ = harness.dispatch(Command::Close {});
    assert_eq!(
        harness.saved("/media/a.mkv").map(|entry| entry.position),
        Some(90.0)
    );
}

#[test]
fn a_failed_save_is_reported_once_until_one_succeeds() {
    let root = unique_temp_dir("okp-session-history-save-failed");
    // A file where the data directory should be makes every save fail.
    let data = root.path().join("data");
    fs::write(&data, b"not a directory").expect("blocking file");
    let harness = Harness::new(&data);
    harness.open("/media/a.mkv");
    harness.load(600.0, vec![]);
    let save_at = |position: f64| {
        let _ = harness.dispatch(Command::SetPaused { paused: false });
        harness.play_to(position);
        let _ = harness.dispatch(Command::SetPaused { paused: true });
        harness.writer.settle();
        // The session collects what the writer did with its next look.
        harness.publish(|_| {}, vec![]);
        harness
            .history_events()
            .into_iter()
            .filter(|event| matches!(event, HistoryEvent::SaveFailed { .. }))
            .count()
    };
    assert_eq!(save_at(120.0), 1);
    assert_eq!(save_at(130.0), 0, "a failure is reported once");

    // Once a save succeeds, the next failure is news again.
    fs::remove_file(&data).expect("unblock the data directory");
    assert_eq!(save_at(140.0), 0);
    assert_eq!(
        saved(&data, "/media/a.mkv").map(|entry| entry.position),
        Some(140.0)
    );
    fs::remove_dir_all(&data).expect("remove the data directory");
    fs::write(&data, b"not a directory").expect("block it again");
    assert_eq!(save_at(150.0), 1);
}

fn fixed_now() -> i64 {
    // 2023-11-14 22:13:20 UTC
    1_700_000_000
}

#[test]
fn the_history_page_groups_entries_by_local_day_and_searches() {
    let root = unique_temp_dir("okp-session-history-page");
    let media = root.path().join("Shows").join("Season 01");
    fs::create_dir_all(&media).expect("media directory");
    let data = root.path().join("data");
    let mut history = History::default();
    let mut keys = Vec::new();
    for (name, updated_at_unix, position) in [
        // 00:56 on the 15th, three hours east of UTC.
        ("one.mkv", 1_699_999_000, 300.0),
        // 22:26 on the 14th there; still the 14th in UTC as well.
        ("two.mkv", 1_699_990_000, 0.0),
        ("three.mkv", 1_699_700_000, 1_000.0),
        ("four.mkv", 1_690_000_000, 20.0),
    ] {
        let path = media.join(name);
        fs::write(&path, b"media").expect("media fixture");
        let key = path.to_string_lossy().into_owned();
        history.files.insert(
            key.clone(),
            FileEntry {
                position,
                duration: 1_200.0,
                updated_at_unix,
                ..FileEntry::default()
            },
        );
        keys.push(key);
    }
    fs::create_dir_all(&data).expect("data directory");
    fs::write(
        history_path(&data),
        serde_json::to_vec_pretty(&history).expect("history serialises"),
    )
    .expect("history fixture");
    let harness = Harness::with_store(
        HistoryStore::open_path_with_clock(history_path(&data), fixed_now),
        &data,
    );

    let (_, sections) = harness.page("", 3 * 3_600);
    let summary: Vec<_> = sections
        .iter()
        .map(|section| {
            (
                section.bucket,
                section.header.as_str(),
                section
                    .entries
                    .iter()
                    .map(|entry| (entry.title.as_str(), entry.when_label.as_str()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (HistoryBucket::Today, "TODAY", vec![("one", "Today 00:56")]),
            (
                HistoryBucket::Yesterday,
                "YESTERDAY",
                vec![("two", "Yest. 22:26")]
            ),
            (
                HistoryBucket::EarlierThisWeek,
                "EARLIER THIS WEEK",
                vec![("three", "Sat 13:53")]
            ),
            (HistoryBucket::Earlier, "EARLIER", vec![("four", "22 Jul")]),
        ]
    );
    let one = &sections[0].entries[0];
    assert_eq!(one.location, "Shows › Season 01");
    assert_eq!(one.state, HistoryState::Progress);
    assert_eq!(one.state_label, "15m left");
    assert_eq!(one.resume_from, Some(300.0));
    assert_eq!(sections[1].entries[0].resume_from, None);
    assert_eq!(sections[3].entries[0].state, HistoryState::Barely);

    // In UTC the first two are both on the 14th.
    let (_, utc) = harness.page("", 0);
    assert_eq!(utc[0].bucket, HistoryBucket::Today);
    assert_eq!(utc[0].entries.len(), 2);

    let (_, found) = harness.page("THREE", 0);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].entries[0].source.location, keys[2]);

    let shelf = harness.session.query(&crate::protocol::Query::History {
        query: HistoryQuery::WelcomeShelf {
            limit: 2,
            utc_offset_seconds: 0,
        },
    });
    let crate::protocol::QueryResult::History {
        result: HistoryQueryResult::WelcomeShelf { entries, .. },
    } = shelf
    else {
        panic!("the shelf query answered {shelf:?}")
    };
    let titles: Vec<_> = entries.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(titles, vec!["one", "two"]);
}

/// The Linux build's history file, with its media under `/home/example/Videos`.
fn linux_history_fixture() -> String {
    fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/linux-history.json"
    ))
    .expect("the Linux history fixture")
}

#[test]
fn a_history_file_written_by_the_linux_build_loads_and_keeps_its_fields() {
    let fixture = linux_history_fixture();
    let parsed = History::load(&fixture).expect("the Linux document loads");
    assert_eq!(
        serde_json::to_vec_pretty(&parsed).expect("history serialises"),
        fixture.as_bytes(),
        "the fixture is exactly what the Linux store writes, and so is ours"
    );

    let root = unique_temp_dir("okp-session-history-linux-file");
    let media = root.path().join("Videos");
    let episode = media.join("Severance/Season 02/Severance S02E03.mkv");
    fs::create_dir_all(episode.parent().expect("episode folder")).expect("media folders");
    for path in [
        episode.clone(),
        media.join("barely.webm"),
        media.join("finished.mp4"),
    ] {
        fs::write(path, b"media").expect("media fixture");
    }
    let data = root.path().join("data");
    fs::create_dir_all(&data).expect("data directory");
    let media_root = media.to_string_lossy().into_owned();
    fs::write(
        history_path(&data),
        fixture.replace("/home/example/Videos", &media_root),
    )
    .expect("history fixture");
    let episode_key = episode.to_string_lossy().into_owned();

    let harness = Harness::new(&data);
    let (_, sections) = harness.page("", 0);
    let entries: Vec<_> = sections
        .iter()
        .flat_map(|section| section.entries.iter())
        .map(|entry| (entry.title.as_str(), entry.state, entry.resume_from))
        .collect();
    assert_eq!(
        entries,
        vec![
            (
                "Severance — Who Is Alive?",
                HistoryState::Progress,
                Some(1325.5)
            ),
            ("Example talk", HistoryState::Progress, Some(300.0)),
            ("finished", HistoryState::Finished, None),
            ("barely", HistoryState::Barely, None),
        ]
    );

    harness.open(&episode_key);
    assert_eq!(resume_of(&harness.last_open()), Some(1325.5));
    harness.load(3120.0, {
        let mut tracks = two_of_each();
        tracks.push(track(okp_mpv::TrackKind::Subtitle, 3));
        tracks
    });
    assert_eq!(
        harness.forwarded()[1..],
        [
            Command::SetSpeed { speed: 1.25 },
            Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(2),
            },
            Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: None,
            },
            Command::SetSubtitleDelay { seconds: -0.35 },
        ]
    );

    harness.play_to(1400.0);
    let _ = harness.dispatch(Command::SetPaused { paused: true });
    let written = String::from_utf8(harness.file()).expect("history file");
    let reloaded = History::load(&written).expect("the Linux store loads what we write");
    let entry = &reloaded.files[&episode_key];
    let original = &parsed.files["/home/example/Videos/Severance/Season 02/Severance S02E03.mkv"];
    assert_eq!(entry.position, 1400.0);
    assert_eq!(entry.title, original.title);
    assert_eq!(entry.bookmarks, original.bookmarks);
    assert_eq!(entry.preferences, original.preferences);
    assert_eq!(reloaded.files.len(), parsed.files.len());
}

#[test]
fn local_time_follows_the_offset_across_days_and_years() {
    assert_eq!(
        local_datetime(1_700_000_000, 0),
        LocalDateTime::new(2023, 11, 14, 22, 13)
    );
    assert_eq!(
        local_datetime(1_700_000_000, 2 * 3_600),
        LocalDateTime::new(2023, 11, 15, 0, 13)
    );
    assert_eq!(
        local_datetime(1_704_067_199, -5 * 3_600),
        LocalDateTime::new(2023, 12, 31, 18, 59)
    );
    assert_eq!(local_datetime(0, 0), LocalDateTime::new(1970, 1, 1, 0, 0));
    assert_eq!(
        local_datetime(951_782_400, 0),
        LocalDateTime::new(2000, 2, 29, 0, 0)
    );
}
