use super::*;

pub(crate) fn update_up_next_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    chrome: &ChromeVisibility,
) {
    let snapshot = {
        let state = state.borrow();
        let has_media = has_loaded_media_state(&state);
        let chapters = state
            .mpv
            .as_ref()
            .map(Mpv::chapters)
            .and_then(Result::ok)
            .unwrap_or_default();

        SidePanelSnapshot {
            has_media,
            current_file: state.current_file.clone(),
            current_url: state.current_url.clone(),
            playlist: state.playlist.items().to_vec(),
            chapters,
            ab_loop: state.ab_loop,
        }
    };

    {
        let mut state = state.borrow_mut();
        if state.chapters_snapshot != snapshot.chapters {
            state.chapters_snapshot = snapshot.chapters.clone();
        }
    }

    controls.chapters_button.set_sensitive(snapshot.has_media);
    if !snapshot.has_media {
        set_side_panel_user_visible(
            &controls.up_next_revealer,
            &controls.chapters_button,
            &controls.side_panel_user_visible,
            &controls.side_panel_pinned,
            chrome,
            false,
        );
        controls.side_panel_snapshot.replace(snapshot);
        controls.side_panel_actions.borrow_mut().clear();
        update_timeline_marks(
            &controls.seek,
            &controls.timeline_marks_snapshot,
            &[],
            AbLoopState::default(),
        );
        clear_list_box(&controls.up_next_list);
        return;
    }

    let panel_visible = controls.side_panel_user_visible.get();
    controls.up_next_revealer.set_visible(panel_visible);
    if panel_visible {
        controls.chapters_button.add_css_class("is-selected");
    } else {
        controls.chapters_button.remove_css_class("is-selected");
    }

    let previous_snapshot = controls.side_panel_snapshot.borrow().clone();
    request_chapter_thumbnail_warm(controls, state, &snapshot);

    if previous_snapshot == snapshot {
        return;
    }

    if panel_visible
        && !controls.side_panel_manual_mode.get()
        && previous_snapshot.chapters.is_empty()
        && !snapshot.chapters.is_empty()
    {
        controls.side_panel_mode.set(SidePanelMode::Chapters);
    }

    controls.side_panel_snapshot.replace(snapshot.clone());
    update_timeline_marks(
        &controls.seek,
        &controls.timeline_marks_snapshot,
        &snapshot.chapters,
        snapshot.ab_loop,
    );

    let current_index = snapshot.playlist.iter().position(|item| {
        item.is_current(
            snapshot.current_file.as_deref(),
            snapshot.current_url.as_deref(),
        )
    });

    let mode = controls.side_panel_mode.get();
    update_side_panel_tab_labels(
        &controls.chapters_tab,
        &controls.up_next_tab,
        snapshot.chapters.len(),
        snapshot.playlist.len(),
    );
    update_side_panel_tab_state(&controls.chapters_tab, &controls.up_next_tab, mode);
    controls.up_next_title.set_text(match mode {
        SidePanelMode::Chapters => "Chapters",
        SidePanelMode::UpNext => "Up Next",
    });
    controls
        .up_next_summary
        .set_text(&side_panel_summary(&snapshot));
    clear_list_box(&controls.up_next_list);
    let mut actions = Vec::new();

    match mode {
        SidePanelMode::Chapters => render_chapters_panel(controls, &snapshot, &mut actions),
        SidePanelMode::UpNext => {
            render_playlist_panel(controls, state, &snapshot, current_index, &mut actions)
        }
    }

    controls.side_panel_actions.replace(actions);
}

pub(crate) fn render_chapters_panel(
    controls: &Controls,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    if snapshot.chapters.is_empty() {
        controls
            .up_next_list
            .append(&panel_empty_row("No chapters in this media yet."));
        actions.push(SidePanelAction::None);
        return;
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "Chapters · {}",
        snapshot.chapters.len()
    )));
    actions.push(SidePanelAction::None);

    for chapter in &snapshot.chapters {
        let thumbnail = snapshot
            .current_file
            .as_ref()
            .and_then(|path| thumbnails::existing_thumbnail_path(path, chapter));
        controls
            .up_next_list
            .append(&chapter_row(chapter, thumbnail));
        actions.push(SidePanelAction::Chapter(chapter.time));
    }
}

pub(crate) fn render_playlist_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
    current_index: Option<usize>,
    actions: &mut Vec<SidePanelAction>,
) {
    if snapshot.playlist.len() <= 1 {
        controls
            .up_next_list
            .append(&panel_empty_row("No folder queue for this media yet."));
        actions.push(SidePanelAction::None);
        return;
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "Up Next · {}",
        snapshot.playlist.len()
    )));
    actions.push(SidePanelAction::None);

    for (index, item) in snapshot.playlist.iter().enumerate() {
        controls.up_next_list.append(&playlist_row(
            item,
            index,
            current_index,
            snapshot.playlist.len(),
            Rc::clone(state),
        ));
        actions.push(SidePanelAction::Playlist(index));
    }
}

pub(crate) fn drain_thumbnail_events(controls: &Controls) {
    let mut changed = false;
    while controls.thumbnail_events.borrow().try_recv().is_ok() {
        changed = true;
    }

    if changed {
        controls
            .side_panel_snapshot
            .replace(SidePanelSnapshot::default());
    }
}

pub(crate) fn request_chapter_thumbnail_warm(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
) {
    let Some(media_path) = snapshot.current_file.as_ref() else {
        return;
    };
    if snapshot.chapters.is_empty() {
        return;
    }

    let key = thumbnails::request_key(media_path, &snapshot.chapters);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.thumbnail_request_key.as_deref() == Some(key.as_str()) {
            false
        } else {
            state.thumbnail_request_key = Some(key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_chapter_thumbnails(
            media_path.clone(),
            snapshot.chapters.clone(),
            key,
            controls.thumbnail_sender.clone(),
        );
    }
}

pub(crate) fn update_timeline_marks(
    seek: &gtk::Scale,
    snapshot: &RefCell<Vec<TimelineMark>>,
    chapters: &[Chapter],
    ab_loop: AbLoopState,
) {
    let marks = timeline_marks(chapters, ab_loop);
    if *snapshot.borrow() == marks {
        return;
    }

    seek.clear_marks();
    for mark in &marks {
        let (position, label) = match mark.kind {
            TimelineMarkKind::Chapter => (gtk::PositionType::Top, None),
            TimelineMarkKind::AbStart => (gtk::PositionType::Bottom, Some("A")),
            TimelineMarkKind::AbEnd => (gtk::PositionType::Bottom, Some("B")),
            TimelineMarkKind::AbLoop => (gtk::PositionType::Bottom, Some("A-B")),
        };
        seek.add_mark(mark.time, position, label);
    }
    snapshot.replace(marks);
}

pub(crate) fn timeline_marks(chapters: &[Chapter], ab_loop: AbLoopState) -> Vec<TimelineMark> {
    let mut marks = chapters
        .iter()
        .map(|chapter| TimelineMark {
            time: chapter.time,
            kind: TimelineMarkKind::Chapter,
        })
        .filter(|mark| mark.time.is_finite() && mark.time > 0.0)
        .collect::<Vec<_>>();

    let ab_start = ab_loop.a.filter(|time| time.is_finite() && *time >= 0.0);
    let ab_end = ab_loop.b.filter(|time| time.is_finite() && *time >= 0.0);
    match (ab_start, ab_end) {
        (Some(a), Some(b)) if should_combine_ab_loop_marks(a, b) => marks.push(TimelineMark {
            time: a + ((b - a) / 2.0),
            kind: TimelineMarkKind::AbLoop,
        }),
        (Some(a), Some(b)) => {
            marks.push(TimelineMark {
                time: a,
                kind: TimelineMarkKind::AbStart,
            });
            marks.push(TimelineMark {
                time: b,
                kind: TimelineMarkKind::AbEnd,
            });
        }
        (Some(time), None) => marks.push(TimelineMark {
            time,
            kind: TimelineMarkKind::AbStart,
        }),
        (None, Some(time)) => marks.push(TimelineMark {
            time,
            kind: TimelineMarkKind::AbEnd,
        }),
        (None, None) => {}
    }

    marks
}

pub(crate) fn should_combine_ab_loop_marks(a: f64, b: f64) -> bool {
    (a - b).abs() <= AB_LOOP_COMBINED_MARK_EPSILON_SECS
}

pub(crate) fn panel_heading_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-heading-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-heading");
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

pub(crate) fn panel_empty_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-empty-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-empty");
    label.set_wrap(true);
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

pub(crate) fn chapter_row(chapter: &Chapter, thumbnail: Option<PathBuf>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_selectable(false);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let thumbnail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    thumbnail_box.add_css_class("okp-chapter-thumb");
    thumbnail_box.set_size_request(88, 50);
    if let Some(thumbnail) = thumbnail {
        let picture = gtk::Picture::for_filename(thumbnail);
        picture.set_size_request(88, 50);
        picture.set_can_shrink(true);
        thumbnail_box.append(&picture);
    }

    let title_text = chapter
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);

    let time = gtk::Label::new(Some(&time_code::format_clock(chapter.time)));
    time.add_css_class("okp-up-next-marker");
    time.set_xalign(0.0);

    let title = gtk::Label::new(Some(&title_text));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    label_box.append(&time);
    label_box.append(&title);
    row_box.append(&thumbnail_box);
    row_box.append(&label_box);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn clear_list_box(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

pub(crate) fn playlist_row(
    item: &PlaylistItem,
    index: usize,
    current_index: Option<usize>,
    playlist_len: usize,
    state: Rc<RefCell<PlayerState>>,
) -> gtk::ListBoxRow {
    let is_current = current_index == Some(index);
    let is_next = current_index.is_some_and(|current| index == current + 1);
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_activatable(!is_current);
    row.set_selectable(false);
    row.set_tooltip_text(Some(&item.display_location()));
    if is_current {
        row.add_css_class("is-current");
    }

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let marker = gtk::Label::new(Some(if is_current {
        "Now"
    } else if is_next {
        "Next"
    } else {
        ""
    }));
    marker.add_css_class("okp-up-next-marker");
    marker.set_width_chars(4);
    marker.set_xalign(0.0);

    let title = gtk::Label::new(Some(&item.display_name()));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    let drag_handle = playlist_drag_handle();

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    actions.add_css_class("okp-up-next-actions");

    let move_up = playlist_action_button("go-up-symbolic", "Move up", index > 0);
    let move_up_state = Rc::clone(&state);
    move_up.connect_clicked(move |_| {
        move_playlist_item(&move_up_state, index, index.saturating_sub(1));
    });
    actions.append(&move_up);

    let play_next_sensitive =
        current_index.is_some_and(|current| index != current && index != current + 1);
    let play_next = playlist_action_button(
        "media-skip-forward-symbolic",
        "Play next",
        play_next_sensitive,
    );
    let play_next_state = Rc::clone(&state);
    play_next.connect_clicked(move |_| {
        if let Some(current) = current_index {
            let target = if index < current {
                current
            } else {
                current + 1
            };
            move_playlist_item(&play_next_state, index, target);
        }
    });
    actions.append(&play_next);

    let move_down =
        playlist_action_button("go-down-symbolic", "Move down", index + 1 < playlist_len);
    let move_down_state = Rc::clone(&state);
    move_down.connect_clicked(move |_| {
        move_playlist_item(&move_down_state, index, index + 1);
    });
    actions.append(&move_down);

    let remove = playlist_action_button(
        "list-remove-symbolic",
        "Remove from queue",
        !is_current && playlist_len > 1,
    );
    let remove_state = Rc::clone(&state);
    remove.connect_clicked(move |_| {
        remove_playlist_item(&remove_state, index);
    });
    actions.append(&remove);

    connect_playlist_row_drag_reorder(&row, &drag_handle, index, state);

    row_box.append(&drag_handle);
    row_box.append(&marker);
    row_box.append(&title);
    row_box.append(&actions);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn playlist_drag_handle() -> gtk::Box {
    let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    handle.add_css_class("okp-up-next-drag-handle");
    handle.set_tooltip_text(Some("Drag to reorder"));
    handle.set_valign(gtk::Align::Center);
    handle.set_can_target(true);

    let icon = gtk::Image::from_icon_name("view-more-symbolic");
    icon.add_css_class("okp-up-next-drag-handle-icon");
    handle.append(&icon);

    handle
}

pub(crate) fn playlist_action_button(
    icon_name: &str,
    tooltip: &str,
    sensitive: bool,
) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("okp-up-next-action-button");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_sensitive(sensitive);
    button
}

pub(crate) fn connect_playlist_row_drag_reorder(
    row: &gtk::ListBoxRow,
    handle: &impl IsA<gtk::Widget>,
    index: usize,
    state: Rc<RefCell<PlayerState>>,
) {
    let drag = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    drag.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&(index as u32).to_value()))
    });
    handle.add_controller(drag);

    let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    let enter_row = row.clone();
    drop.connect_enter(move |_, _, _| {
        enter_row.add_css_class("is-drop-target");
        gdk::DragAction::MOVE
    });
    let leave_row = row.clone();
    drop.connect_leave(move |_| {
        leave_row.remove_css_class("is-drop-target");
    });
    let drop_row = row.clone();
    drop.connect_drop(move |_, value, _, y| {
        drop_row.remove_css_class("is-drop-target");
        let Ok(source_index) = value.get::<u32>() else {
            return false;
        };
        let drop_after = y >= f64::from(drop_row.allocated_height()) / 2.0;
        let Some(target_index) =
            playlist_drop_target_index(source_index as usize, index, drop_after)
        else {
            return false;
        };
        move_playlist_item(&state, source_index as usize, target_index)
    });
    row.add_controller(drop);
}

pub(crate) fn playlist_drop_target_index(
    source_index: usize,
    row_index: usize,
    drop_after: bool,
) -> Option<usize> {
    if source_index == row_index {
        return None;
    }

    let target = match (drop_after, source_index < row_index) {
        (false, true) => row_index.saturating_sub(1),
        (false, false) => row_index,
        (true, true) => row_index,
        (true, false) => row_index + 1,
    };

    (target != source_index).then_some(target)
}
