//! The playlist's decisions without a session. The queue tests are the GTK
//! shell's `playlist_ops::queue_tests`, lifted onto the session playlist.

use super::*;
use crate::protocol::SourceKind;

const CLIP: &str = "https://clips.example/cam/C0001.MP4?token=abc";

fn local(path: &str) -> PlaylistItem {
    PlaylistItem::Local(PathBuf::from(path))
}

fn clip() -> PlaylistItem {
    PlaylistItem::Url(CLIP.to_owned())
}

fn source(item: &PlaylistItem) -> MediaSource {
    MediaSource::from_item(item)
}

fn accepted() -> DispatchResult {
    DispatchResult::Accepted { request_id: 1 }
}

fn quiet() -> SessionPlaylist {
    SessionPlaylist::new(Arc::new(|| {}))
}

/// A playlist playing `/media/current.mkv` within `items`.
fn playing_local(items: Vec<PlaylistItem>) -> (SessionPlaylist, PlaylistItem) {
    let current = local("/media/current.mkv");
    let mut playlist = quiet();
    playlist.list = Playlist::from_items(items, Some(&current), false);
    (playlist, current)
}

fn queue(
    playlist: &mut SessionPlaylist,
    current: Option<&PlaylistItem>,
    items: Vec<PlaylistItem>,
    mode: QueueMode,
) -> Step {
    playlist.dispatch(
        &PlaylistAction::Queue {
            sources: items.iter().map(source).collect(),
            mode,
        },
        current,
    )
}

fn applied() -> Step {
    Step::Reply(DispatchResult::Applied {})
}

fn no_op() -> Step {
    Step::Reply(DispatchResult::NoOp {})
}

/// Carry out what the playlist planned, the way the session does, and return
/// what was opened.
fn carry_out(playlist: &mut SessionPlaylist, step: Step) -> Option<PlaylistItem> {
    let Step::Open(open) = step else { return None };
    let item = open.item.clone();
    playlist.opened(open, &accepted());
    Some(item)
}

#[test]
fn appended_url_is_reached_by_auto_advance() {
    let (mut playlist, current) = playing_local(vec![local("/media/current.mkv")]);

    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![clip()],
            QueueMode::Append
        ),
        applied()
    );
    assert_eq!(playlist.list.items(), [local("/media/current.mkv"), clip()]);
    let state = playlist.state();
    assert_eq!(
        state.items[1].title, "C0001.MP4",
        "Up Next labels a queued clip by its file name"
    );
    assert_eq!(state.items[1].source.kind, SourceKind::Url);

    // The current file ends: auto-advance reaches the queued URL.
    playlist.end_of_file();
    let steps = playlist.poll(Some(&current));
    let [Background::Open(open)] = steps.as_slice() else {
        panic!("expected one open, got {steps:?}")
    };
    assert_eq!(open.item, clip());
    assert_eq!(
        open.command(),
        Command::Open {
            source: source(&clip()),
            resume_from: None,
            audio_track: None,
            subtitle_track: None,
        }
    );
    playlist.opened(open.clone(), &accepted());
    assert_eq!(playlist.list.current(), Some(&clip()));
    assert_eq!(playlist.list.items(), [local("/media/current.mkv"), clip()]);
}

#[test]
fn play_next_url_is_next_and_opens_when_chosen_in_up_next() {
    let (mut playlist, current) =
        playing_local(vec![local("/media/current.mkv"), local("/media/later.mkv")]);

    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![clip()],
            QueueMode::PlayNext
        ),
        applied()
    );
    assert_eq!(
        playlist.list.items(),
        [
            local("/media/current.mkv"),
            clip(),
            local("/media/later.mkv")
        ]
    );
    assert_eq!(playlist.list.peek_next(), Some(&clip()));

    let step = playlist.dispatch(&PlaylistAction::Jump { index: 1 }, Some(&current));
    assert_eq!(carry_out(&mut playlist, step), Some(clip()));
    assert_eq!(playlist.list.current_index(), Some(1));
}

#[test]
fn queue_request_reports_why_nothing_was_queued() {
    let mut idle = quiet();
    assert!(matches!(
        queue(&mut idle, None, vec![clip()], QueueMode::Append),
        Step::Reply(DispatchResult::Rejected {
            reason: RejectReason::NoActiveMedia,
            ..
        })
    ));
    assert!(idle.list.items().is_empty());

    let (mut playlist, current) = playing_local(vec![local("/media/current.mkv")]);
    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![
                local("/media/readme.txt"),
                PlaylistItem::Url("not a url".to_owned()),
            ],
            QueueMode::Append,
        ),
        no_op(),
        "nothing queueable"
    );
    // Repeats inside one request count once.
    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![clip(), clip()],
            QueueMode::Append
        ),
        applied()
    );
    for mode in [QueueMode::Append, QueueMode::PlayNext] {
        assert_eq!(
            queue(
                &mut playlist,
                Some(&current),
                vec![local("/media/current.mkv")],
                mode
            ),
            no_op(),
            "already playing"
        );
    }
    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![clip()],
            QueueMode::Append
        ),
        no_op(),
        "already queued"
    );
    assert_eq!(playlist.list.items(), [local("/media/current.mkv"), clip()]);
}

#[test]
fn only_http_urls_join_the_queue_never_engine_protocols_or_nested_requests() {
    let (mut playlist, current) = playing_local(vec![local("/media/current.mkv")]);
    let hostile = [
        "edl://clip.mp4,0,10;clip.mp4,20,10",
        "lavfi://testsrc",
        "memory://data",
        "smb://nas/share/clip.mp4",
        "rtsp://camera.example/stream",
        "ok-player://open?url=https%3A%2F%2Fclips.example%2Fa.mp4",
        "https://user@clips.example/clip.mp4",
    ];

    for mode in [QueueMode::Append, QueueMode::PlayNext] {
        for url in hostile {
            assert_eq!(
                queue(
                    &mut playlist,
                    Some(&current),
                    vec![PlaylistItem::Url(url.to_owned())],
                    mode
                ),
                no_op(),
                "{url} must not be queued ({mode:?})"
            );
        }
    }
    assert_eq!(playlist.list.items(), [local("/media/current.mkv")]);

    // A mixed request keeps the http(s) clip and drops the rest, so advancing can
    // only ever reach the clip.
    let mut request = hostile
        .map(|url| PlaylistItem::Url(url.to_owned()))
        .to_vec();
    request.insert(3, clip());
    assert_eq!(
        queue(&mut playlist, Some(&current), request, QueueMode::Append),
        applied()
    );
    assert_eq!(playlist.list.items(), [local("/media/current.mkv"), clip()]);
}

#[test]
fn local_file_requests_keep_the_menu_action_semantics() {
    let (mut playlist, current) = playing_local(vec![
        local("/media/current.mkv"),
        local("/media/queued.mkv"),
    ]);

    // Non-media entries and repeats are dropped before the queue sees them.
    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![
                local("/media/notes.txt"),
                local("/media/new.mp4"),
                local("/media/new.mp4"),
                local("/media/queued.mkv"),
                local("/media/album.flac"),
            ],
            QueueMode::Append,
        ),
        applied()
    );
    assert_eq!(
        playlist.list.items(),
        [
            local("/media/current.mkv"),
            local("/media/queued.mkv"),
            local("/media/new.mp4"),
            local("/media/album.flac"),
        ]
    );

    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![local("/media/album.flac")],
            QueueMode::PlayNext
        ),
        applied()
    );
    assert_eq!(
        playlist.list.items(),
        [
            local("/media/current.mkv"),
            local("/media/album.flac"),
            local("/media/queued.mkv"),
            local("/media/new.mp4"),
        ]
    );
    assert_eq!(playlist.list.current_index(), Some(0));
}

#[test]
fn an_end_without_an_auto_advance_target_plays_nothing() {
    let (mut playlist, current) = playing_local(vec![local("/media/current.mkv")]);
    playlist.list.set_auto_advance(false);
    playlist.end_of_file();
    assert!(playlist.poll(Some(&current)).is_empty());

    let (mut playlist, current) =
        playing_local(vec![local("/media/first.mkv"), local("/media/current.mkv")]);
    playlist.end_of_file();
    assert!(
        playlist.poll(Some(&current)).is_empty(),
        "the last item with repeat off"
    );
    // The flag is spent: a later poll does not advance either.
    playlist.list.set_repeat(CoreRepeatMode::All);
    assert!(playlist.poll(Some(&current)).is_empty());
}

#[test]
fn a_finished_job_that_is_no_longer_in_flight_is_ignored() {
    let (mut playlist, _) = playing_local(vec![local("/media/current.mkv")]);
    playlist.next_job = 4;
    playlist.active = Some(ActiveJob {
        id: 4,
        cancel: Arc::new(AtomicBool::new(false)),
        kind: JobKind::Folder,
    });
    let folder = |file: &str, siblings: &[&str]| Outcome::Folder {
        file: PathBuf::from(file),
        siblings: Some(siblings.iter().map(PathBuf::from).collect()),
    };
    playlist.inbox.lock().unwrap().push(Finished {
        job: 3,
        outcome: folder("/old/x.mkv", &["/old/w.mkv", "/old/x.mkv"]),
    });
    assert!(playlist.poll(None).is_empty());
    assert_eq!(playlist.list.items(), [local("/media/current.mkv")]);
    assert!(
        playlist.state().scanning,
        "the newer job is still in flight"
    );

    playlist.inbox.lock().unwrap().push(Finished {
        job: 4,
        outcome: folder(
            "/media/current.mkv",
            &["/media/a.mkv", "/media/current.mkv"],
        ),
    });
    assert!(playlist.poll(None).is_empty());
    assert_eq!(
        playlist.list.items(),
        [local("/media/a.mkv"), local("/media/current.mkv")]
    );
    assert_eq!(playlist.list.current_index(), Some(1));
    assert!(!playlist.state().scanning);
}

#[test]
fn a_folder_merge_keeps_what_was_queued_while_it_was_listed() {
    let (mut playlist, current) = playing_local(vec![local("/media/current.mkv")]);
    assert_eq!(
        queue(
            &mut playlist,
            Some(&current),
            vec![clip()],
            QueueMode::Append
        ),
        applied()
    );
    playlist.merge_folder(
        Path::new("/media/current.mkv"),
        vec![
            PathBuf::from("/media/a.mkv"),
            PathBuf::from("/media/current.mkv"),
            PathBuf::from("/media/z.mkv"),
        ],
    );
    assert_eq!(
        playlist.list.items(),
        [
            local("/media/a.mkv"),
            local("/media/current.mkv"),
            local("/media/z.mkv"),
            clip(),
        ]
    );
    assert_eq!(playlist.list.current(), Some(&current));
}

#[test]
fn every_change_a_shell_can_see_is_reported_once() {
    let (mut playlist, current) =
        playing_local(vec![local("/media/current.mkv"), local("/media/b.mkv")]);
    assert!(playlist.take_changed().is_none(), "nothing changed yet");

    assert_eq!(
        playlist.dispatch(
            &PlaylistAction::SetRepeat {
                mode: RepeatMode::One
            },
            Some(&current)
        ),
        applied()
    );
    assert_eq!(
        playlist.dispatch(
            &PlaylistAction::SetRepeat {
                mode: RepeatMode::One
            },
            Some(&current)
        ),
        no_op()
    );
    let reported = playlist.take_changed().expect("the repeat mode changed");
    assert_eq!(reported.repeat, RepeatMode::One);
    assert!(playlist.take_changed().is_none());

    assert_eq!(
        playlist.dispatch(&PlaylistAction::Move { from: 5, to: 0 }, Some(&current)),
        no_op()
    );
    assert!(
        playlist.take_changed().is_none(),
        "a refused edit is silent"
    );
}
