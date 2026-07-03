use super::*;

pub(crate) fn open_media_dialog(parent: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open media"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);
    dialog.add_filter(&media_file_filter());
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&subtitle_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            load_selected_local_paths(&state, file_chooser_paths(dialog));
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn open_folder_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open folder"),
        Some(parent),
        gtk::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && !load_selected_local_paths(&state, file_chooser_paths(dialog))
        {
            status_toast.show("Folder has no playable media");
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn open_url_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Open URL")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Open", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Open URL"));

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://example.com/video.mkv"));
    entry.set_activates_default(true);
    entry.set_width_chars(52);
    content.append(&entry);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let url = entry.text().trim().to_owned();
            if media_formats::is_playable_url(Some(&url)) {
                load_media_url(&state, url);
            } else {
                status_toast.show("Enter a valid stream URL");
            }
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn open_go_to_time_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let Some((position, duration)) = go_to_time_snapshot(&state) else {
        status_toast.show("Open media first");
        return;
    };

    let dialog = gtk::Dialog::builder()
        .title("Go to Time")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Go", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Go to Time"));

    let label = gtk::Label::new(Some("Enter a timecode or seconds."));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    content.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&time_code::format(position));
    entry.set_placeholder_text(Some("1:23 or 90"));
    entry.set_activates_default(true);
    entry.set_width_chars(18);
    content.append(&entry);

    let range = if duration.is_finite() && duration > 0.0 {
        format!("Duration {}", time_code::format(duration))
    } else {
        "Duration unknown".to_owned()
    };
    let hint = gtk::Label::new(Some(&range));
    hint.add_css_class("okp-info-label");
    hint.set_xalign(0.0);
    content.append(&hint);

    let focus_entry = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response != gtk::ResponseType::Accept {
            dialog.close();
            return;
        }

        let text = entry.text();
        let Some(mut target) = time_code::parse(Some(text.as_str())) else {
            entry.add_css_class("is-error");
            status_toast.show("Enter a valid timecode");
            return;
        };

        if let Some((_, duration)) = go_to_time_snapshot(&state) {
            if duration.is_finite() && duration > 0.0 {
                target = target.min(duration);
            }
        } else {
            status_toast.show("Open media first");
            dialog.close();
            return;
        }

        if seek_to_time(&state, target) {
            status_toast.show(&format!("Jumped to {}", time_code::format(target)));
            dialog.close();
        } else {
            status_toast.show("Could not seek");
        }
    });

    dialog.present();
    focus_entry.grab_focus();
    focus_entry.select_region(0, -1);
}

pub(crate) fn go_to_time_snapshot(state: &Rc<RefCell<PlayerState>>) -> Option<(f64, f64)> {
    let state = state.borrow();
    if !has_loaded_media_state(&state) {
        return None;
    }

    let playback = state
        .mpv
        .as_ref()
        .map(|mpv| mpv.observed_playback_state())?;
    let position = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    Some((position, duration))
}

pub(crate) fn open_clear_history_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Clear History")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Clear", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(14);
    content.set_margin_end(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.append(&command_dialog_title("Clear History"));

    let message = gtk::Label::new(Some(
        "Clear saved resume positions and per-file playback preferences?",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    content.append(&message);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            clear_history(&state, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn command_dialog_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-command-dialog-title");
    label.set_xalign(0.0);
    label
}

pub(crate) fn captionless_transient_window(
    parent: &gtk::ApplicationWindow,
    title: &str,
    default_width: i32,
    default_height: i32,
    resizable: bool,
) -> gtk::Window {
    let window = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .default_width(default_width)
        .default_height(default_height)
        .resizable(resizable)
        .decorated(false)
        .build();
    window.set_decorated(false);
    window
}

pub(crate) fn open_subtitle_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Add subtitle"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Add", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.add_filter(&subtitle_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_subtitle_path(&state, path);
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn open_playlist_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open playlist"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_m3u_playlist(&state, &path, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn save_playlist_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Save playlist"),
        Some(parent),
        gtk::FileChooserAction::Save,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Save", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_current_name("OK Player Playlist.m3u");
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            save_m3u_playlist(&state, playlist_save_path(path), &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn open_queue_media_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    mode: QueueInsertMode,
) {
    let (title, accept_label) = match mode {
        QueueInsertMode::Append => ("Add to Queue", "Add"),
        QueueInsertMode::PlayNext => ("Play Next", "Add"),
    };
    let dialog = gtk::FileChooserDialog::new(
        Some(title),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            (accept_label, gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);
    dialog.add_filter(&media_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            queue_media_paths(&state, file_chooser_paths(dialog), mode, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

pub(crate) fn file_chooser_paths(dialog: &gtk::FileChooserDialog) -> Vec<PathBuf> {
    let files = dialog.files();
    let mut paths = Vec::new();
    for index in 0..files.n_items() {
        if let Some(path) = files
            .item(index)
            .and_then(|object| object.downcast::<gtk::gio::File>().ok())
            .and_then(|file| file.path())
        {
            paths.push(path);
        }
    }

    if paths.is_empty()
        && let Some(path) = dialog.file().and_then(|file| file.path())
    {
        paths.push(path);
    }

    paths
}

pub(crate) fn playlist_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("M3U playlists"));
    filter.add_pattern("*.m3u");
    filter.add_pattern("*.m3u8");
    filter
}

pub(crate) fn media_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Media files"));
    for extension in media_formats::extensions() {
        let pattern = format!("*{extension}");
        filter.add_pattern(&pattern);
        filter.add_pattern(&pattern.to_ascii_uppercase());
    }
    filter
}

pub(crate) fn subtitle_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Subtitle files"));
    for extension in media_formats::SUBTITLE_EXTENSIONS {
        let pattern = format!("*{extension}");
        filter.add_pattern(&pattern);
        filter.add_pattern(&pattern.to_ascii_uppercase());
    }
    filter
}

pub(crate) fn all_files_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("All files"));
    filter.add_pattern("*");
    filter
}

pub(crate) fn connect_drop(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    empty_surface: EmptySurface,
) {
    let drop_target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let enter_surface = empty_surface.clone();
    drop_target.connect_enter(move |_, _, _| {
        enter_surface.set_drop_active(true);
        gdk::DragAction::COPY
    });
    let leave_surface = empty_surface.clone();
    drop_target.connect_leave(move |_| {
        leave_surface.set_drop_active(false);
    });
    let drop_surface = empty_surface;
    drop_target.connect_drop(move |_, value, _, _| {
        drop_surface.set_drop_active(false);
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };

        load_selected_local_paths(&state, dropped_file_list_paths(&files))
    });
    window.add_controller(drop_target);
}

pub(crate) fn dropped_file_list_paths(files: &gdk::FileList) -> Vec<PathBuf> {
    files
        .files()
        .into_iter()
        .filter_map(|file| file.path())
        .collect()
}

pub(crate) fn load_selected_local_paths(
    state: &Rc<RefCell<PlayerState>>,
    paths: Vec<PathBuf>,
) -> bool {
    let media_paths = selected_media_paths(&paths);
    match media_paths.as_slice() {
        [path] => {
            load_media_path(state, path.clone());
            load_selected_subtitles(state, selected_subtitle_paths(&paths));
            return true;
        }
        [] => {}
        _ => {
            let playlist = media_paths
                .into_iter()
                .map(PlaylistItem::Local)
                .collect::<Vec<_>>();
            let Some(first_item) = playlist.first().cloned() else {
                return false;
            };
            let loaded = load_playlist_item_with_playlist(state, first_item, playlist, true);
            if loaded {
                load_selected_subtitles(state, selected_subtitle_paths(&paths));
            }
            return loaded;
        }
    }

    if let Some(path) = selected_playlist_path(&paths) {
        return load_m3u_playlist_silent(state, &path);
    }

    load_selected_subtitles(state, selected_subtitle_paths(&paths))
}

pub(crate) fn selected_media_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut media_paths = Vec::new();
    for path in paths {
        if path.is_dir() {
            media_paths.extend(media_paths_in_directory(path));
        } else if is_media_path(path) {
            media_paths.push(path.clone());
        }
    }
    unique_media_paths(media_paths)
}

pub(crate) fn selected_subtitle_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut subtitles = Vec::new();
    for path in paths {
        if is_subtitle_path(path) && !subtitles.iter().any(|existing| existing == path) {
            subtitles.push(path.clone());
        }
    }
    subtitles
}

pub(crate) fn selected_playlist_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| is_playlist_path(path)).cloned()
}

pub(crate) fn load_selected_subtitles(
    state: &Rc<RefCell<PlayerState>>,
    paths: Vec<PathBuf>,
) -> bool {
    let mut loaded = false;
    for path in paths {
        loaded |= load_subtitle_path(state, path);
    }
    loaded
}
