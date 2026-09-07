use super::*;
use okp_core::history::trash_unloads_current_source;

/// Shell seam for the one destructive History command. Production uses GIO's recoverable
/// desktop Trash operation; tests inject a deterministic adapter. There is intentionally no
/// permanent-delete fallback.
pub(crate) trait NativeTrash {
    fn move_to_trash(&self, path: &Path) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GioNativeTrash;

impl NativeTrash for GioNativeTrash {
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        gtk::gio::File::for_path(path)
            .trash(None::<&gtk::gio::Cancellable>)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TrashHistoryResult {
    Cancelled,
    NotLocal,
    TrashFailed(String),
    Moved {
        unload_current: bool,
        history_removed: bool,
    },
    MovedButHistorySaveFailed {
        unload_current: bool,
        error: String,
    },
}

/// Execute the confirmed operation in its irreversible order: native Trash first, History
/// persistence second. A native failure changes no History state. A History save failure after
/// Trash is necessarily partial and remains distinguishable so the UI never claims full success.
pub(crate) fn move_history_source_to_trash(
    history: &mut history::HistoryStore,
    source: &PlaylistItem,
    current_source: Option<&PlaylistItem>,
    confirmed: bool,
    trash: &impl NativeTrash,
) -> TrashHistoryResult {
    if !confirmed {
        return TrashHistoryResult::Cancelled;
    }
    let PlaylistItem::Local(path) = source else {
        return TrashHistoryResult::NotLocal;
    };

    if let Err(error) = trash.move_to_trash(path) {
        return TrashHistoryResult::TrashFailed(error);
    }

    let unload_current = trash_unloads_current_source(source, current_source);
    match history.remove_source_persisted(source, unload_current) {
        Ok(history_removed) => TrashHistoryResult::Moved {
            unload_current,
            history_removed,
        },
        Err(error) => {
            // The media has already left its original location. Even though the transactional
            // History removal rolled back, an active engine session must not repopulate it from
            // a late save while the shell is stopping and unloading that source.
            if unload_current {
                history.suppress_source(source);
            }
            TrashHistoryResult::MovedButHistorySaveFailed {
                unload_current,
                error: error.to_string(),
            }
        }
    }
}

pub(crate) fn remove_history_source(
    surface: &EmptySurface,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
    source: &PlaylistItem,
) {
    let outcome = {
        let mut state = state.borrow_mut();
        let active = current_history_source(&state).as_ref() == Some(source);
        let outcome = state.history.remove_source_persisted(source, active);
        if matches!(outcome, Ok(true)) && active {
            state.pending_resume = None;
            state.pending_preferences = None;
        }
        outcome
    };

    match outcome {
        Ok(true) => {
            surface.refresh(parent, state, Rc::clone(status_toast));
            status_toast.show("Removed from history");
        }
        Ok(false) => {
            surface.refresh(parent, state, Rc::clone(status_toast));
            status_toast.show("Already removed from history");
        }
        Err(error) => {
            eprintln!("Failed to persist History removal: {error}");
            status_toast.show("Could not save History; item was not removed");
        }
    }
}

#[allow(deprecated)]
pub(crate) fn open_move_to_trash_dialog(
    surface: EmptySurface,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    source: PlaylistItem,
) {
    let PlaylistItem::Local(path) = &source else {
        status_toast.show("Only local files can be moved to Trash");
        return;
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("this file");
    let title = format!("Move “{file_name}” to Trash?");
    let dialog = gtk::Dialog::builder()
        .title(&title)
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Move to Trash", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(14);
    content.set_margin_end(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.append(&command_dialog_title(&title));
    let message = gtk::Label::new(Some(
        "The file can be recovered from your desktop Trash. Sidecars and other media stay in place.",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    content.append(&message);

    let parent = parent.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            apply_confirmed_trash(
                &surface,
                &parent,
                &state,
                &status_toast,
                &source,
                &GioNativeTrash,
            );
        }
        dialog.close();
    });
    dialog.present();
}

fn apply_confirmed_trash(
    surface: &EmptySurface,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
    source: &PlaylistItem,
    trash: &impl NativeTrash,
) {
    let result = {
        let mut state = state.borrow_mut();
        let current = current_history_source(&state);
        move_history_source_to_trash(&mut state.history, source, current.as_ref(), true, trash)
    };

    match result {
        TrashHistoryResult::Moved { unload_current, .. } => {
            reconcile_player_after_trash(state, source, unload_current);
            surface.refresh(parent, state, Rc::clone(status_toast));
            status_toast.show("Moved to Trash and removed from history");
        }
        TrashHistoryResult::MovedButHistorySaveFailed {
            unload_current,
            error,
        } => {
            reconcile_player_after_trash(state, source, unload_current);
            surface.refresh(parent, state, Rc::clone(status_toast));
            eprintln!("File moved to Trash, but History persistence failed: {error}");
            status_toast.show("Moved to Trash, but could not save History");
        }
        TrashHistoryResult::TrashFailed(error) => {
            eprintln!("Native Trash operation failed: {error}");
            status_toast.show("Could not move file to Trash; check file permissions");
        }
        TrashHistoryResult::Cancelled => {}
        TrashHistoryResult::NotLocal => {
            status_toast.show("Only local files can be moved to Trash");
        }
    }
}

fn reconcile_player_after_trash(
    state: &Rc<RefCell<PlayerState>>,
    source: &PlaylistItem,
    unload_current: bool,
) {
    if unload_current {
        let stop_result = {
            let state = state.borrow();
            state.mpv.as_ref().map(Mpv::stop)
        };
        if let Some(Err(error)) = stop_result {
            // The file is already in Trash, so the logical session must still retire even if
            // libmpv refuses its stop command. Advancing the generation drops late callbacks.
            eprintln!("Failed to stop media after moving it to Trash: {error}");
        }
        let mut playlist = state.borrow().playlist.clone();
        playlist.remove_unavailable_source(source);
        clear_loaded_media_state(state);
        state.borrow_mut().playlist = playlist;
        return;
    }

    state
        .borrow_mut()
        .playlist
        .remove_unavailable_source(source);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeTrash<'a> {
        calls: &'a Cell<usize>,
        result: Result<(), &'static str>,
    }

    impl NativeTrash for FakeTrash<'_> {
        fn move_to_trash(&self, _path: &Path) -> Result<(), String> {
            self.calls.set(self.calls.get() + 1);
            self.result.map_err(str::to_owned)
        }
    }

    fn seeded_store(path: PathBuf, source: &PlaylistItem) -> history::HistoryStore {
        let mut history = history::HistoryStore::open_test(path);
        history.record_source_with_title(
            source,
            30.0,
            300.0,
            false,
            false,
            okp_core::nfo_metadata::HistoryTitleUpdate::Preserve,
        );
        history
    }

    #[test]
    fn cancellation_and_native_failure_leave_history_unchanged() {
        let root = tempfile::tempdir().expect("temporary history directory");
        let source = PlaylistItem::Local(root.path().join("movie.mkv"));
        let mut history = seeded_store(root.path().join("history.json"), &source);
        let calls = Cell::new(0);
        let successful = FakeTrash {
            calls: &calls,
            result: Ok(()),
        };

        assert_eq!(
            move_history_source_to_trash(&mut history, &source, None, false, &successful),
            TrashHistoryResult::Cancelled
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(history.search("").len(), 1);

        let failing = FakeTrash {
            calls: &calls,
            result: Err("trash unavailable"),
        };
        assert_eq!(
            move_history_source_to_trash(&mut history, &source, None, true, &failing),
            TrashHistoryResult::TrashFailed("trash unavailable".to_owned())
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(history.search("").len(), 1);
        assert!(!history.is_source_suppressed(&source));
    }

    #[test]
    fn url_never_reaches_the_native_trash_adapter() {
        let root = tempfile::tempdir().expect("temporary history directory");
        let source = PlaylistItem::Url("https://cdn.example.test/tmp/movie.mkv".to_owned());
        let mut history = seeded_store(root.path().join("history.json"), &source);
        let calls = Cell::new(0);
        let trash = FakeTrash {
            calls: &calls,
            result: Ok(()),
        };

        assert_eq!(
            move_history_source_to_trash(&mut history, &source, Some(&source), true, &trash),
            TrashHistoryResult::NotLocal
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(history.search("").len(), 1);
    }

    #[test]
    fn history_failure_after_trash_is_reported_as_partial_and_suppresses_active_source() {
        let source = PlaylistItem::Local(PathBuf::from("/media/movie.mkv"));
        let mut history = seeded_store(PathBuf::from("/dev/null/history.json"), &source);
        let calls = Cell::new(0);
        let trash = FakeTrash {
            calls: &calls,
            result: Ok(()),
        };

        let result =
            move_history_source_to_trash(&mut history, &source, Some(&source), true, &trash);
        assert!(matches!(
            result,
            TrashHistoryResult::MovedButHistorySaveFailed {
                unload_current: true,
                ..
            }
        ));
        assert_eq!(calls.get(), 1);
        assert_eq!(history.search("").len(), 1);
        assert!(history.is_source_suppressed(&source));
    }

    #[test]
    fn automatic_playlist_loads_preserve_removal_guard_for_local_and_url() {
        let root = tempfile::tempdir().expect("temporary history directory");
        let local = root.path().join("movie.mp4");
        fs::write(&local, b"fixture").expect("local media identity");
        for (index, source) in [
            PlaylistItem::Local(local),
            PlaylistItem::Url("https://example.test/watch/803".to_owned()),
        ]
        .into_iter()
        .enumerate()
        {
            let mut history =
                seeded_store(root.path().join(format!("history-{index}.json")), &source);
            history.remove_source_persisted(&source, true).unwrap();
            let state = Rc::new(RefCell::new(PlayerState {
                history,
                ..PlayerState::default()
            }));

            // The actual EOF/repeat call chain must preserve the removal guard, including
            // the underlying path/URL loader and remember-loaded callbacks.
            assert!(load_playlist_item_with_playlist(
                &state,
                source.clone(),
                vec![source.clone()],
                false,
            ));
            assert!(state.borrow().history.is_source_suppressed(&source));
            state.borrow_mut().history.record_source_opened(
                &source,
                None,
                false,
                okp_core::nfo_metadata::HistoryTitleUpdate::Preserve,
            );
            assert!(state.borrow().history.search("").is_empty());

            // A user-driven playlist selection follows the same route with explicit intent.
            assert!(load_playlist_item_with_playlist(
                &state,
                source.clone(),
                vec![source.clone()],
                true,
            ));
            assert!(!state.borrow().history.is_source_suppressed(&source));
        }
    }

    #[test]
    fn successful_trash_unloads_only_matching_playback_and_prunes_inactive_queue_item() {
        let current = PlaylistItem::Local(PathBuf::from("/media/current.mkv"));
        let trashed = PlaylistItem::Local(PathBuf::from("/media/queued.mkv"));
        let retained = PlaylistItem::Local(PathBuf::from("/media/retained.mkv"));
        let state = Rc::new(RefCell::new(PlayerState {
            current_file: match &current {
                PlaylistItem::Local(path) => Some(path.clone()),
                PlaylistItem::Url(_) => None,
            },
            playlist: Playlist::from_items(
                vec![current.clone(), trashed.clone(), retained.clone()],
                Some(&current),
                false,
            ),
            ..PlayerState::default()
        }));

        reconcile_player_after_trash(&state, &trashed, false);
        {
            let state = state.borrow();
            assert_eq!(current_history_source(&state), Some(current.clone()));
            assert_eq!(state.playlist.items(), &[current.clone(), retained.clone()]);
        }

        reconcile_player_after_trash(&state, &current, true);
        let state = state.borrow();
        assert!(current_history_source(&state).is_none());
        assert_eq!(state.playlist.items(), std::slice::from_ref(&retained));
        assert_eq!(state.playlist.current_index(), None);
    }
}
