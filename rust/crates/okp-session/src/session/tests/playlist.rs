use super::*;
use crate::playlist::scan::Filesystem;
use crate::protocol::{PlaylistAction, PlaylistState, QueueMode, RepeatMode};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Holds a directory read until the test opens it, or for ten seconds at most:
/// a read made on a test's own thread (a scan that blocks its caller) must
/// fail that test, not hang it.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    condvar: Condvar,
}

impl Gate {
    fn wait(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut open = lock(&self.open);
        while !*open {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            open = self
                .condvar
                .wait_timeout(open, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    fn release(&self) {
        *lock(&self.open) = true;
        self.condvar.notify_all();
    }
}

/// Opens the gate when dropped, so a failing assertion never leaves a thread
/// waiting on it.
struct ReleaseOnDrop(Arc<Gate>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// Folders and playlist files in memory. A gated folder or file is read only
/// once its gate opens: a slow or sleeping network mount.
#[derive(Default)]
struct FakeFolders {
    folders: HashMap<PathBuf, Vec<PathBuf>>,
    files: HashMap<PathBuf, String>,
    gates: HashMap<PathBuf, Arc<Gate>>,
    /// Reads that have started.
    reading: AtomicUsize,
    /// Folder listings that the scan has let go of.
    released: AtomicUsize,
}

impl FakeFolders {
    fn folder(mut self, dir: &str, names: &[&str]) -> Self {
        let dir = PathBuf::from(dir);
        let entries = names.iter().map(|name| dir.join(name)).collect();
        self.folders.insert(dir, entries);
        self
    }

    fn file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(PathBuf::from(path), text.to_owned());
        self
    }

    fn gated(mut self, path: &str) -> (Self, Arc<Gate>) {
        let gate = Arc::new(Gate::default());
        self.gates.insert(PathBuf::from(path), Arc::clone(&gate));
        (self, gate)
    }

    fn hold(&self, path: &Path) {
        self.reading.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = self.gates.get(path) {
            gate.wait();
        }
    }
}

/// A listing that counts itself let go of.
struct Listing<'a> {
    entries: std::vec::IntoIter<PathBuf>,
    released: &'a AtomicUsize,
}

impl Iterator for Listing<'_> {
    type Item = PathBuf;

    fn next(&mut self) -> Option<PathBuf> {
        self.entries.next()
    }
}

impl Drop for Listing<'_> {
    fn drop(&mut self) {
        self.released.fetch_add(1, Ordering::SeqCst);
    }
}

impl Filesystem for FakeFolders {
    fn read_dir(&self, dir: &Path) -> io::Result<Box<dyn Iterator<Item = PathBuf> + '_>> {
        self.hold(dir);
        let entries = self
            .folders
            .get(dir)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Box::new(Listing {
            entries: entries.into_iter(),
            released: &self.released,
        }))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.folders.contains_key(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.hold(path);
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
}

fn session_with(folders: FakeFolders) -> (Session, Arc<FakeState>, Arc<FakeFolders>) {
    let (session, engine) = fake_session();
    let folders = Arc::new(folders);
    lock(&session.shared.state)
        .playlist
        .set_filesystem(Arc::clone(&folders) as Arc<dyn Filesystem>);
    (session, engine, folders)
}

fn act(session: &Session, action: PlaylistAction) -> DispatchResult {
    session.dispatch(&Command::Playlist { action })
}

fn source(path: &str) -> MediaSource {
    MediaSource {
        kind: SourceKind::Local,
        location: path.to_owned(),
    }
}

fn playlist(session: &Session) -> PlaylistState {
    match session.query(&Query::Playlist {}) {
        QueryResult::Playlist { playlist } => playlist,
        other => panic!("expected the playlist, got {other:?}"),
    }
}

fn locations(state: &PlaylistState) -> Vec<String> {
    state
        .items
        .iter()
        .map(|entry| entry.source.location.clone())
        .collect()
}

/// Wait until the playlist has these items and its scan has finished.
fn settled_on(session: &Session, expected: &[&str]) -> PlaylistState {
    let mut state = playlist(session);
    let settled = wait_for(|| {
        state = playlist(session);
        !state.scanning && locations(&state) == expected
    });
    assert!(
        settled,
        "the playlist never settled on {expected:?}: {state:?}"
    );
    state
}

/// Every location the engine was asked to open, in order.
fn opened(engine: &FakeState) -> Vec<String> {
    engine
        .forwarded()
        .into_iter()
        .filter_map(|command| match command {
            Command::Open { source, .. } => Some(source.location),
            _ => None,
        })
        .collect()
}

/// The playlist events of everything queued so far. The query first folds in
/// whatever the event thread has not processed yet.
fn drained_playlists(session: &Session) -> Vec<PlaylistState> {
    let _ = session.snapshot();
    session
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::PlaylistChanged { playlist } => Some(playlist),
            _ => None,
        })
        .collect()
}

fn end_of_file(engine: &FakeState) {
    engine.publish(|_| {}, vec![ended(EndFileReason::Eof)]);
}

fn playing(session: &Session, engine: &FakeState) {
    loaded(engine, 10.0);
    assert!(wait_for(|| session.snapshot().status == Status::Playing));
}

#[test]
fn a_local_open_plays_at_once_while_a_slow_folder_scan_runs_and_fills_up_next_after() {
    let (folders, gate) = FakeFolders::default()
        .folder(
            "/media",
            &[
                "Episode 10.mkv",
                "Episode 2.mkv",
                "notes.txt",
                "Episode 1.mkv",
            ],
        )
        .gated("/media");
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let (session, engine, folders) = session_with(folders);

    // The open must answer while the folder read is still held.
    let (sender, receiver) = mpsc::channel();
    let reply = thread::scope(|scope| {
        scope.spawn(|| sender.send(session.dispatch(&open("/media/Episode 2.mkv"))));
        receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| {
                // Let the blocked open finish so the scope can end.
                gate.release();
                panic!("the open did not return while the folder read was held")
            })
    });
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "{reply:?}"
    );
    assert!(
        wait_for(|| folders.reading.load(Ordering::SeqCst) == 1),
        "the folder scan started"
    );
    assert_eq!(opened(&engine), ["/media/Episode 2.mkv"]);
    let scanning = playlist(&session);
    assert_eq!(locations(&scanning), ["/media/Episode 2.mkv"]);
    assert_eq!(scanning.current_index, Some(0));
    assert!(scanning.scanning);

    // Playback starts before the scan completes.
    playing(&session, &engine);
    assert!(playlist(&session).scanning, "the scan is still held");

    gate.release();
    let filled = settled_on(
        &session,
        &[
            "/media/Episode 1.mkv",
            "/media/Episode 2.mkv",
            "/media/Episode 10.mkv",
        ],
    );
    assert_eq!(filled.current_index, Some(1));
    assert_eq!(filled.items[1].title, "Episode 2.mkv");
    assert_eq!(
        drained_playlists(&session).last(),
        Some(&filled),
        "the playlist event reports the filled folder"
    );
    assert_eq!(
        opened(&engine),
        ["/media/Episode 2.mkv"],
        "nothing reopened"
    );
}

#[test]
fn a_folder_scan_superseded_by_a_newer_open_is_dropped() {
    let (folders, gate) = FakeFolders::default()
        .folder("/a", &["0.mkv", "1.mkv", "2.mkv"])
        .folder("/b", &["y.mkv", "x.mkv"])
        .gated("/a");
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let (session, _engine, folders) = session_with(folders);

    let _ = session.dispatch(&open("/a/1.mkv"));
    assert!(wait_for(|| folders.reading.load(Ordering::SeqCst) == 1));
    let _ = session.dispatch(&open("/b/y.mkv"));
    let newer = settled_on(&session, &["/b/x.mkv", "/b/y.mkv"]);
    assert_eq!(newer.current_index, Some(1));

    // The older scan finishes after all; what it found never shows.
    gate.release();
    assert!(
        wait_for(|| folders.released.load(Ordering::SeqCst) == 2),
        "the older scan let go of its listing"
    );
    thread::sleep(Duration::from_millis(50));
    assert_eq!(playlist(&session), newer);
    assert!(
        drained_playlists(&session)
            .iter()
            .all(|state| locations(state)
                .iter()
                .all(|item| item != "/a/0.mkv" && item != "/a/2.mkv")),
        "no playlist event carries the superseded folder"
    );
}

#[test]
fn closing_empties_the_playlist_and_drops_the_scan_in_flight() {
    let (folders, gate) = FakeFolders::default()
        .folder("/media", &["a.mkv", "b.mkv"])
        .gated("/media");
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let (session, engine, folders) = session_with(folders);
    let _ = session.dispatch(&open("/media/a.mkv"));
    assert!(wait_for(|| folders.reading.load(Ordering::SeqCst) == 1));
    playing(&session, &engine);

    assert!(matches!(
        session.dispatch(&Command::Close {}),
        DispatchResult::Accepted { .. }
    ));
    let closed = playlist(&session);
    assert!(closed.items.is_empty() && closed.current_index.is_none() && !closed.scanning);

    gate.release();
    assert!(wait_for(|| folders.released.load(Ordering::SeqCst) == 1));
    thread::sleep(Duration::from_millis(50));
    assert_eq!(playlist(&session), closed);
}

#[test]
fn the_end_of_a_file_plays_the_next_item_and_the_last_item_stops() {
    let folders = FakeFolders::default().folder("/media", &["b.mkv", "a.mkv"]);
    let (session, engine, _) = session_with(folders);
    let _ = session.dispatch(&open("/media/a.mkv"));
    settled_on(&session, &["/media/a.mkv", "/media/b.mkv"]);
    playing(&session, &engine);

    end_of_file(&engine);
    assert!(
        wait_for(|| opened(&engine) == ["/media/a.mkv", "/media/b.mkv"]),
        "{:?}",
        opened(&engine)
    );
    let snapshot = session.snapshot();
    assert_eq!(
        snapshot.source.map(|source| source.location).as_deref(),
        Some("/media/b.mkv")
    );
    assert_eq!(snapshot.status, Status::Opening);
    assert_eq!(playlist(&session).current_index, Some(1));

    // Repeat off: the last item ends and nothing else plays.
    playing(&session, &engine);
    end_of_file(&engine);
    assert!(wait_for(|| session.snapshot().status == Status::Ended));
    thread::sleep(Duration::from_millis(30));
    assert_eq!(opened(&engine).len(), 2);

    // Repeat all wraps to the first item.
    assert_eq!(
        act(
            &session,
            PlaylistAction::SetRepeat {
                mode: RepeatMode::All
            }
        ),
        DispatchResult::Applied {}
    );
    let _ = act(&session, PlaylistAction::Jump { index: 1 });
    playing(&session, &engine);
    end_of_file(&engine);
    assert!(wait_for(|| opened(&engine).last().map(String::as_str)
        == Some("/media/a.mkv")
        && opened(&engine).len() == 4));
    assert_eq!(playlist(&session).current_index, Some(0));
}

#[test]
fn repeat_one_restarts_the_item_and_a_failed_end_advances_nothing() {
    let folders = FakeFolders::default().folder("/media", &["a.mkv", "b.mkv"]);
    let (session, engine, _) = session_with(folders);
    let _ = session.dispatch(&open("/media/a.mkv"));
    settled_on(&session, &["/media/a.mkv", "/media/b.mkv"]);
    playing(&session, &engine);
    assert_eq!(
        act(
            &session,
            PlaylistAction::SetRepeat {
                mode: RepeatMode::One
            }
        ),
        DispatchResult::Applied {}
    );
    assert_eq!(playlist(&session).repeat, RepeatMode::One);

    end_of_file(&engine);
    assert!(
        wait_for(|| opened(&engine) == ["/media/a.mkv", "/media/a.mkv"]),
        "{:?}",
        opened(&engine)
    );
    assert_eq!(session.snapshot().status, Status::Opening);
    assert_eq!(playlist(&session).current_index, Some(0));

    // A file that fails at its end is not followed by anything.
    playing(&session, &engine);
    engine.publish(|_| {}, vec![ended(EndFileReason::Error(-13))]);
    assert!(wait_for(|| session.snapshot().status == Status::Ended));
    thread::sleep(Duration::from_millis(30));
    assert_eq!(opened(&engine).len(), 2);
}

#[test]
fn previous_and_next_wrap_at_both_ends_whatever_the_repeat_mode() {
    let folders = FakeFolders::default().folder("/media", &["a.mkv", "b.mkv", "c.mkv"]);
    let (session, engine, _) = session_with(folders);
    let _ = session.dispatch(&open("/media/a.mkv"));
    settled_on(&session, &["/media/a.mkv", "/media/b.mkv", "/media/c.mkv"]);

    assert!(matches!(
        act(&session, PlaylistAction::Previous {}),
        DispatchResult::Accepted { .. }
    ));
    assert_eq!(playlist(&session).current_index, Some(2));
    assert!(matches!(
        act(&session, PlaylistAction::Next {}),
        DispatchResult::Accepted { .. }
    ));
    assert_eq!(playlist(&session).current_index, Some(0));
    assert_eq!(
        act(&session, PlaylistAction::Jump { index: 3 }),
        DispatchResult::NoOp {}
    );
    assert_eq!(
        opened(&engine),
        ["/media/a.mkv", "/media/c.mkv", "/media/a.mkv"]
    );

    // A single item has nowhere to step to.
    let alone = FakeFolders::default().folder("/solo", &["only.mkv"]);
    let (session, engine, _) = session_with(alone);
    let _ = session.dispatch(&open("/solo/only.mkv"));
    settled_on(&session, &["/solo/only.mkv"]);
    assert_eq!(
        act(&session, PlaylistAction::Next {}),
        DispatchResult::NoOp {}
    );
    assert_eq!(opened(&engine).len(), 1);
}

#[test]
fn opening_several_files_plays_them_in_the_given_order_without_a_scan() {
    let (session, engine, folders) = session_with(FakeFolders::default());
    let reply = act(
        &session,
        PlaylistAction::Open {
            subtitles: Vec::new(),
            sources: vec![
                source("/media/b.mkv"),
                source("/media/subs.srt"),
                source("/media/a.mp4"),
                source("/media/b.mkv"),
            ],
        },
    );
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "{reply:?}"
    );
    assert_eq!(opened(&engine), ["/media/b.mkv"]);
    let list = playlist(&session);
    assert_eq!(locations(&list), ["/media/b.mkv", "/media/a.mp4"]);
    assert_eq!(list.current_index, Some(0));
    assert!(!list.scanning);
    assert_eq!(folders.reading.load(Ordering::SeqCst), 0);

    assert_eq!(
        act(
            &session,
            PlaylistAction::Open {
                subtitles: Vec::new(),
                sources: vec![source("/media/subs.srt")]
            }
        ),
        DispatchResult::NoOp {},
        "nothing playable"
    );
}

#[test]
fn an_m3u_open_is_read_off_the_callers_thread_and_plays_its_entries() {
    let (folders, gate) = FakeFolders::default()
        .file(
            "/lists/shows/queue.m3u8",
            "#EXTM3U\n#EXTINF:1,Two\nep2.mkv\nhttps://example.test/ep3.mp4\n/abs/ep1.mkv\nnotes.txt\n",
        )
        .gated("/lists/shows/queue.m3u8");
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let (session, engine, folders) = session_with(folders);
    let reply = act(
        &session,
        PlaylistAction::Open {
            subtitles: Vec::new(),
            sources: vec![source("/lists/shows/queue.m3u8")],
        },
    );
    assert_eq!(reply, DispatchResult::Applied {});
    assert!(wait_for(|| folders.reading.load(Ordering::SeqCst) == 1));
    assert!(playlist(&session).scanning);
    assert!(opened(&engine).is_empty(), "nothing plays until it is read");

    gate.release();
    let list = settled_on(
        &session,
        &[
            "/lists/shows/ep2.mkv",
            "https://example.test/ep3.mp4",
            "/abs/ep1.mkv",
        ],
    );
    assert_eq!(list.items[1].source.kind, SourceKind::Url);
    assert_eq!(list.items[1].title, "ep3.mp4");
    assert_eq!(list.current_index, Some(0));
    assert_eq!(opened(&engine), ["/lists/shows/ep2.mkv"]);
}

#[test]
fn opening_a_folder_plays_its_media_and_an_empty_one_reports_an_error() {
    let folders = FakeFolders::default()
        .folder("/shows", &["Episode 2.mkv", "Episode 1.mkv", "cover.jpg"])
        .folder("/empty", &["cover.jpg"]);
    let (session, engine, _) = session_with(folders);
    assert_eq!(
        act(
            &session,
            PlaylistAction::Open {
                subtitles: Vec::new(),
                sources: vec![source("/shows")]
            }
        ),
        DispatchResult::Applied {}
    );
    let list = settled_on(&session, &["/shows/Episode 1.mkv", "/shows/Episode 2.mkv"]);
    assert_eq!(list.current_index, Some(0));
    assert_eq!(opened(&engine), ["/shows/Episode 1.mkv"]);

    session.drain_events();
    let _ = act(
        &session,
        PlaylistAction::Open {
            subtitles: Vec::new(),
            sources: vec![source("/empty")],
        },
    );
    let mut errors = Vec::new();
    assert!(wait_for(|| {
        let _ = session.snapshot();
        errors.extend(
            session
                .drain_events()
                .into_iter()
                .filter_map(|event| match event {
                    Event::Error { kind, message, .. } => Some((kind, message)),
                    _ => None,
                }),
        );
        !errors.is_empty()
    }));
    assert_eq!(
        errors,
        [(
            ErrorKind::NothingPlayable,
            "nothing playable was found".to_owned()
        )]
    );
    assert_eq!(opened(&engine).len(), 1, "the current media keeps playing");
    assert_eq!(locations(&playlist(&session)).len(), 2);
}

#[test]
fn queue_edits_answer_applied_and_coalesce_into_one_playlist_event() {
    let folders = FakeFolders::default().folder("/m", &["a.mkv", "b.mkv", "c.mkv"]);
    let (session, engine, _) = session_with(folders);
    let _ = session.dispatch(&open("/m/a.mkv"));
    settled_on(&session, &["/m/a.mkv", "/m/b.mkv", "/m/c.mkv"]);
    playing(&session, &engine);
    drained_playlists(&session);

    let reply = session.dispatch_json(
        r#"{"schema_version":1,"command":{"playlist":{"action":{"move":{"from":2,"to":0}}}}}"#,
    );
    assert_eq!(reply, r#"{"schema_version":1,"result":{"applied":{}}}"#);
    assert_eq!(
        act(
            &session,
            PlaylistAction::Queue {
                sources: vec![source("/new/d.mp4"), source("/new/readme.txt")],
                mode: QueueMode::PlayNext,
            }
        ),
        DispatchResult::Applied {}
    );
    let events = drained_playlists(&session);
    assert_eq!(events.len(), 1, "one event per batch: {events:?}");
    assert_eq!(
        locations(&events[0]),
        ["/m/c.mkv", "/m/a.mkv", "/new/d.mp4", "/m/b.mkv"]
    );
    assert_eq!(events[0].current_index, Some(1));

    assert_eq!(
        act(&session, PlaylistAction::PlayNext { index: 3 }),
        DispatchResult::Applied {}
    );
    assert_eq!(
        act(&session, PlaylistAction::Remove { index: 0 }),
        DispatchResult::Applied {}
    );
    assert_eq!(
        locations(&playlist(&session)),
        ["/m/a.mkv", "/m/b.mkv", "/new/d.mp4"]
    );
    assert_eq!(
        act(&session, PlaylistAction::Remove { index: 0 }),
        DispatchResult::NoOp {},
        "the current item stays"
    );
    assert_eq!(
        act(&session, PlaylistAction::ClearQueue {}),
        DispatchResult::Applied {}
    );
    assert_eq!(locations(&playlist(&session)), ["/m/a.mkv"]);
    assert_eq!(
        opened(&engine),
        ["/m/a.mkv"],
        "edits never reach the engine"
    );

    // Without media there is nothing to queue around.
    let _ = session.dispatch(&Command::Close {});
    assert!(matches!(
        act(
            &session,
            PlaylistAction::Queue {
                sources: vec![source("/new/e.mp4")],
                mode: QueueMode::Append,
            }
        ),
        DispatchResult::Rejected {
            reason: RejectReason::NoActiveMedia,
            ..
        }
    ));
}

#[test]
fn shuffle_with_a_seed_steps_through_the_same_order_every_time() {
    let names = ["1.mkv", "2.mkv", "3.mkv", "4.mkv", "5.mkv", "6.mkv"];
    let order = |seed: u64| {
        let (session, engine, _) = session_with(FakeFolders::default().folder("/album", &names));
        let _ = session.dispatch(&open("/album/1.mkv"));
        assert!(wait_for(
            || locations(&playlist(&session)).len() == names.len()
        ));
        assert_eq!(
            act(
                &session,
                PlaylistAction::SetShuffle {
                    enabled: true,
                    seed: Some(seed),
                }
            ),
            DispatchResult::Applied {}
        );
        assert!(playlist(&session).shuffle);
        for _ in 1..names.len() {
            let _ = act(&session, PlaylistAction::Next {});
        }
        opened(&engine)
    };

    let first = order(7);
    assert_eq!(first, order(7), "the same seed gives the same order");
    let mut visited = first.clone();
    visited.sort();
    visited.dedup();
    assert_eq!(
        visited.len(),
        names.len(),
        "every item plays once: {first:?}"
    );
}

#[test]
fn an_end_of_file_before_the_folder_is_listed_plays_the_next_file_once_it_is() {
    for repeat in [RepeatMode::Off, RepeatMode::All] {
        let (folders, gate) = FakeFolders::default()
            .folder("/m", &["a.mkv", "b.mkv"])
            .gated("/m");
        let _release = ReleaseOnDrop(Arc::clone(&gate));
        let (session, engine, folders) = session_with(folders);
        let _ = act(&session, PlaylistAction::SetRepeat { mode: repeat });
        let _ = session.dispatch(&open("/m/a.mkv"));
        assert!(wait_for(|| folders.reading.load(Ordering::SeqCst) == 1));
        playing(&session, &engine);

        // A short clip ends while its folder is still being listed.
        end_of_file(&engine);
        assert!(wait_for(|| session.snapshot().status == Status::Ended));
        thread::sleep(Duration::from_millis(30));
        assert_eq!(
            opened(&engine),
            ["/m/a.mkv"],
            "{repeat:?}: nothing plays before the folder is listed"
        );
        assert!(playlist(&session).scanning);

        gate.release();
        assert!(
            wait_for(|| opened(&engine) == ["/m/a.mkv", "/m/b.mkv"]),
            "{repeat:?}: the next file of the folder plays: {:?}",
            opened(&engine)
        );
        assert_eq!(
            settled_on(&session, &["/m/a.mkv", "/m/b.mkv"]).current_index,
            Some(1)
        );
    }
}

#[test]
fn an_end_of_file_waiting_for_a_listing_is_dropped_by_a_newer_open() {
    let (folders, gate) = FakeFolders::default()
        .folder("/m", &["a.mkv", "b.mkv"])
        .folder("/n", &["x.mkv", "y.mkv"])
        .gated("/m");
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let (session, engine, _) = session_with(folders);
    let _ = session.dispatch(&open("/m/a.mkv"));
    playing(&session, &engine);
    end_of_file(&engine);
    assert!(wait_for(|| session.snapshot().status == Status::Ended));

    // The user opens something else before the folder of the clip is listed.
    let _ = session.dispatch(&open("/n/x.mkv"));
    settled_on(&session, &["/n/x.mkv", "/n/y.mkv"]);
    gate.release();
    thread::sleep(Duration::from_millis(50));
    let _ = session.snapshot();
    assert_eq!(
        opened(&engine),
        ["/m/a.mkv", "/n/x.mkv"],
        "the end of the clip does not skip the newly opened file"
    );
}

/// A session playing `/a/1.mkv` from the folder playlist `[/a/1.mkv, /a/2.mkv]`
/// whose playlist `open` of the folder `/slow` (holding `slow`) is held in its
/// read: a sleeping network mount.
fn reading_a_slow_open(slow: &[&str]) -> (Session, Arc<FakeState>, Arc<FakeFolders>, Arc<Gate>) {
    let (folders, gate) = FakeFolders::default()
        .folder("/a", &["1.mkv", "2.mkv"])
        .folder("/slow", slow)
        .gated("/slow");
    let (session, engine, folders) = session_with(folders);
    let _ = session.dispatch(&open("/a/1.mkv"));
    settled_on(&session, &["/a/1.mkv", "/a/2.mkv"]);
    playing(&session, &engine);
    assert_eq!(
        act(
            &session,
            PlaylistAction::Open {
                subtitles: Vec::new(),
                sources: vec![source("/slow")],
            }
        ),
        DispatchResult::Applied {}
    );
    assert!(
        wait_for(|| folders.reading.load(Ordering::SeqCst) == 2),
        "the read of /slow started"
    );
    assert!(playlist(&session).scanning);
    (session, engine, folders, gate)
}

#[test]
fn a_jump_while_an_open_is_read_drops_the_read() {
    let (session, engine, folders, gate) = reading_a_slow_open(&["x.mkv", "y.mkv"]);
    let _release = ReleaseOnDrop(Arc::clone(&gate));

    assert!(matches!(
        act(&session, PlaylistAction::Jump { index: 1 }),
        DispatchResult::Accepted { .. }
    ));
    let picked = playlist(&session);
    assert_eq!(locations(&picked), ["/a/1.mkv", "/a/2.mkv"]);
    assert_eq!(picked.current_index, Some(1));
    assert!(!picked.scanning, "the read was dropped");

    gate.release();
    assert!(
        wait_for(|| folders.released.load(Ordering::SeqCst) == 2),
        "the dropped read let go of its listing"
    );
    thread::sleep(Duration::from_millis(50));
    assert_eq!(opened(&engine), ["/a/1.mkv", "/a/2.mkv"]);
    assert_eq!(playlist(&session), picked);
}

#[test]
fn an_end_of_file_while_an_open_is_read_waits_for_the_read() {
    let (session, engine, _, gate) = reading_a_slow_open(&["x.mkv", "y.mkv"]);
    let _release = ReleaseOnDrop(Arc::clone(&gate));

    end_of_file(&engine);
    assert!(wait_for(|| session.snapshot().status == Status::Ended));
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        opened(&engine),
        ["/a/1.mkv"],
        "the playlist being replaced does not advance"
    );

    gate.release();
    settled_on(&session, &["/slow/x.mkv", "/slow/y.mkv"]);
    assert_eq!(opened(&engine), ["/a/1.mkv", "/slow/x.mkv"]);
}

#[test]
fn an_end_of_file_while_an_open_finds_nothing_plays_on_in_the_old_playlist() {
    let (session, engine, _, gate) = reading_a_slow_open(&["cover.jpg"]);
    let _release = ReleaseOnDrop(Arc::clone(&gate));

    end_of_file(&engine);
    assert!(wait_for(|| session.snapshot().status == Status::Ended));
    thread::sleep(Duration::from_millis(30));
    assert_eq!(opened(&engine), ["/a/1.mkv"], "the read decides first");

    gate.release();
    assert!(
        wait_for(|| opened(&engine) == ["/a/1.mkv", "/a/2.mkv"]),
        "{:?}",
        opened(&engine)
    );
    let list = settled_on(&session, &["/a/1.mkv", "/a/2.mkv"]);
    assert_eq!(list.current_index, Some(1));
}

#[test]
fn what_is_queued_while_an_open_is_read_follows_the_opened_items() {
    let (session, engine, _, gate) = reading_a_slow_open(&["x.mkv", "y.mkv"]);
    let _release = ReleaseOnDrop(Arc::clone(&gate));

    for extra in ["/q/extra.mkv", "/q/gone.mkv"] {
        assert_eq!(
            act(
                &session,
                PlaylistAction::Queue {
                    sources: vec![source(extra)],
                    mode: QueueMode::Append,
                }
            ),
            DispatchResult::Applied {}
        );
    }
    assert_eq!(
        act(&session, PlaylistAction::Remove { index: 3 }),
        DispatchResult::Applied {}
    );
    assert_eq!(
        locations(&playlist(&session)),
        ["/a/1.mkv", "/a/2.mkv", "/q/extra.mkv"],
        "the queue shows at once"
    );

    gate.release();
    let list = settled_on(&session, &["/slow/x.mkv", "/slow/y.mkv", "/q/extra.mkv"]);
    assert_eq!(list.current_index, Some(0));
    assert_eq!(opened(&engine), ["/a/1.mkv", "/slow/x.mkv"]);
}

#[test]
fn the_registry_steps_through_the_playlist_once_it_has_two_items() {
    fn steps(session: &Session) -> (bool, bool) {
        let QueryResult::CommandRegistry { commands } = session.query(&Query::CommandRegistry {
            fullscreen: false,
            mini_player: false,
        }) else {
            panic!("the registry query answers with the registry");
        };
        let enabled = |id: &str| {
            commands
                .iter()
                .find(|command| command.id == id)
                .is_some_and(|command| command.enabled)
        };
        (enabled("previous-item"), enabled("next-item"))
    }

    let (session, engine, _folders) = session_with(FakeFolders::default());
    let _ = act(
        &session,
        PlaylistAction::Open {
            subtitles: Vec::new(),
            sources: vec![source("/media/a.mkv")],
        },
    );
    playing(&session, &engine);
    assert_eq!(steps(&session), (false, false), "one item");

    let _ = act(
        &session,
        PlaylistAction::Open {
            subtitles: Vec::new(),
            sources: vec![source("/media/a.mkv"), source("/media/b.mkv")],
        },
    );
    playing(&session, &engine);
    assert_eq!(steps(&session), (true, true), "two items");
}

fn open_with_subtitles(session: &Session, paths: &[&str], subtitles: &[&str]) -> DispatchResult {
    act(
        session,
        PlaylistAction::Open {
            sources: paths.iter().map(|path| source(path)).collect(),
            subtitles: subtitles.iter().map(|path| (*path).to_owned()).collect(),
        },
    )
}

fn added_subtitles(engine: &FakeState) -> Vec<String> {
    engine
        .forwarded()
        .into_iter()
        .filter_map(|command| match command {
            Command::AddSubtitle { path } => Some(path),
            _ => None,
        })
        .collect()
}

#[test]
fn mixed_open_subtitles_wait_for_loaded_media_and_are_not_repeated_on_next() {
    let (session, engine, _) = session_with(FakeFolders::default());
    let reply = open_with_subtitles(
        &session,
        &["/m/a.mkv", "/m/b.mkv"],
        &["/m/a.en.srt", "/m/a.en.srt", "/m/a.de.ass"],
    );
    assert!(matches!(reply, DispatchResult::Accepted { .. }));
    assert!(
        added_subtitles(&engine).is_empty(),
        "never attach while opening"
    );
    loaded(&engine, 30.0);
    assert!(wait_for(|| added_subtitles(&engine).len() == 2));
    assert_eq!(added_subtitles(&engine), ["/m/a.en.srt", "/m/a.de.ass"]);
    // Another FileLoaded or a playlist step never replays the attachment.
    loaded(&engine, 30.0);
    let _ = act(&session, PlaylistAction::Next {});
    loaded(&engine, 30.0);
    assert!(wait_for(|| session.snapshot().status == Status::Playing));
    assert_eq!(added_subtitles(&engine).len(), 2);
}

#[test]
fn mixed_open_subtitles_follow_load_identity_even_when_the_same_path_is_reopened() {
    let (session, engine, _) = session_with(FakeFolders::default());
    engine.report_loads.store(true, Ordering::SeqCst);
    let _ = open_with_subtitles(&session, &["/m/a.mkv"], &["/m/old.srt"]);
    let _ = open_with_subtitles(&session, &["/m/a.mkv"], &["/m/new.srt"]);
    engine.publish_for_loads(
        |_| {},
        vec![(
            MpvEvent::FileLoaded {
                video_dimensions: None,
            },
            Some(1),
        )],
    );
    publish_and_settle(&engine, |_| {});
    assert!(
        added_subtitles(&engine).is_empty(),
        "the older load is not this open"
    );
    engine.publish_for_loads(
        |_| {},
        vec![(
            MpvEvent::FileLoaded {
                video_dimensions: None,
            },
            Some(2),
        )],
    );
    assert!(wait_for(|| added_subtitles(&engine) == ["/m/new.srt"]));
}

#[test]
fn mixed_open_subtitles_are_discarded_by_close_load_failure_or_an_engine_refusal() {
    for failure in ["close", "load", "immediate"] {
        let (session, engine, _) = session_with(FakeFolders::default());
        if failure == "immediate" {
            *lock(&engine.refuse_with) = Some("refused".to_owned());
        }
        let _ = open_with_subtitles(&session, &["/m/a.mkv"], &["/m/old.srt"]);
        if failure == "close" {
            let _ = session.dispatch(&Command::Close {});
        } else if failure == "load" {
            engine.publish(
                |_| {},
                vec![MpvEvent::EndFile {
                    reason: EndFileReason::Error(-13),
                    path: Some("/m/a.mkv".to_owned()),
                    diagnostic_messages: vec![],
                    last_duration: None,
                }],
            );
            assert!(wait_for(|| session.snapshot().status != Status::Opening));
            // Even a delayed Loaded for this failed item must not attach its files.
            loaded(&engine, 30.0);
            assert!(wait_for(|| session.snapshot().status == Status::Playing));
            assert!(added_subtitles(&engine).is_empty());
        }
        let _ = session.dispatch(&open("/m/later.mkv"));
        loaded(&engine, 30.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        assert!(
            added_subtitles(&engine).is_empty(),
            "{failure} left stale subtitles"
        );
    }
}

#[test]
fn mixed_open_subtitles_survive_folder_resolution_but_not_a_superseded_scan() {
    for superseded in [false, true] {
        let (folders, gate) = FakeFolders::default()
            .folder("/show", &["a.mkv", "b.mkv"])
            .gated("/show");
        let _release = ReleaseOnDrop(Arc::clone(&gate));
        let (session, engine, folders) = session_with(folders);
        assert_eq!(
            open_with_subtitles(&session, &["/show"], &["/show/custom.srt"]),
            DispatchResult::Applied {}
        );
        assert!(wait_for(|| folders.reading.load(Ordering::SeqCst) > 0));
        assert!(added_subtitles(&engine).is_empty());
        if superseded {
            let _ = session.dispatch(&open("/later.mkv"));
        }
        gate.release();
        if !superseded {
            assert!(wait_for(
                || opened(&engine).contains(&"/show/a.mkv".to_owned())
            ));
        }
        loaded(&engine, 30.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        if superseded {
            assert!(added_subtitles(&engine).is_empty());
        } else {
            assert!(wait_for(|| added_subtitles(&engine) == ["/show/custom.srt"]));
        }
    }
}

#[test]
fn mixed_open_subtitles_discard_only_the_failed_open_request() {
    for stale in [false, true] {
        let (session, engine, _) = session_with(FakeFolders::default());
        let old = open_with_subtitles(&session, &["/m/a.mkv"], &["/m/old.srt"]);
        let new = open_with_subtitles(&session, &["/m/a.mkv"], &["/m/new.srt"]);
        let DispatchResult::Accepted { request_id: old } = old else {
            panic!("old open rejected")
        };
        let DispatchResult::Accepted { request_id: new } = new else {
            panic!("new open rejected")
        };
        engine.publish(
            |_| {},
            vec![MpvEvent::CommandReply {
                request_id: if stale { old } else { new },
                error: -13,
            }],
        );
        publish_and_settle(&engine, |_| {});
        loaded(&engine, 30.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        if stale {
            assert!(wait_for(|| added_subtitles(&engine) == ["/m/new.srt"]));
        } else {
            assert!(
                added_subtitles(&engine).is_empty(),
                "the failed request kept pending subtitles"
            );
        }
    }
}
