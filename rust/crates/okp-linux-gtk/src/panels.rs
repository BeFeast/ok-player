use super::*;

pub(crate) fn update_up_next_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    chrome: &ChromeVisibility,
) {
    // The visual smoke hook owns the panel while it renders fixture rows, but it
    // must release the moment real media loads: a session that inherited the
    // `OKP_OPEN_SIDE_PANEL_ON_STARTUP` env var has to fall back to live data
    // instead of showing fixtures for the rest of the process.
    if controls.side_panel_preview_frozen.get() {
        let has_real_media = has_loaded_media_state(&state.borrow());
        if has_real_media {
            controls.side_panel_preview_frozen.set(false);
        } else {
            return;
        }
    }

    // Detection state belongs to one media item. Reset it when the source identity changes so
    // an unavailable/error result never leaks into the next file.
    {
        let state = state.borrow();
        let previous = controls.side_panel_snapshot.borrow();
        if previous.current_file != state.current_file || previous.current_url != state.current_url
        {
            controls
                .chapter_detection
                .set(chapter_math::ChapterDetection::default());
        }
    }

    let snapshot = {
        let state = state.borrow();
        let has_media = has_loaded_media_state(&state);
        let chapters = state
            .mpv
            .as_ref()
            .map(Mpv::observed_chapters)
            .unwrap_or_default();
        let playback = state.mpv.as_ref().map(|mpv| mpv.observed_playback_state());
        let position = playback.and_then(|playback| playback.time_pos);
        let duration = playback
            .and_then(|playback| playback.duration)
            .filter(|value| value.is_finite() && *value > 0.0);
        let current_chapter = current_chapter_index(&chapters, position);
        // Only a local file carries persistent per-file bookmarks (streams are not
        // tracked, exactly as history keys off `current_file`).
        let bookmarks = state
            .current_file
            .as_ref()
            .map(|path| state.history.bookmarks(path))
            .unwrap_or_default();

        SidePanelSnapshot {
            has_media,
            current_file: state.current_file.clone(),
            current_url: state.current_url.clone(),
            current_title: has_media
                .then(|| current_media_title(&state))
                .filter(|title| !title.is_empty()),
            playlist: state.playlist.items().to_vec(),
            chapters,
            current_chapter,
            duration,
            bookmarks,
            ab_loop: state.ab_loop,
            detection: controls.chapter_detection.get(),
        }
    };

    {
        let mut state = state.borrow_mut();
        if state.chapters_snapshot != snapshot.chapters {
            state.chapters_snapshot = snapshot.chapters.clone();
        }
    }

    // The length the seek scale is (or will be) ranged to. Timeline marks past the end
    // are dropped so a stray chapter/bookmark can't collapse onto the right handle; a
    // non-positive value means the duration is not known yet.
    let media_duration = snapshot.duration.unwrap_or(0.0);

    controls.chapters_button.set_sensitive(snapshot.has_media);
    if !snapshot.has_media {
        set_side_panel_user_visible(
            &controls.up_next_revealer,
            &controls.side_panel_fade_revealer,
            &controls.chapters_button,
            &controls.side_panel_user_visible,
            &controls.side_panel_pinned,
            chrome,
            false,
        );
        controls.side_panel_snapshot.replace(snapshot);
        controls.side_panel_actions.borrow_mut().clear();
        update_timeline_marks(
            &controls.timeline_rail,
            &[],
            &[],
            &[],
            AbLoopState::default(),
            0.0,
        );
        clear_list_box(&controls.up_next_list);
        return;
    }

    let panel_visible = controls.side_panel_user_visible.get();
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
        && !snapshot_has_chapter_surface(&previous_snapshot)
        && snapshot_has_chapter_surface(&snapshot)
    {
        controls.side_panel_mode.set(SidePanelMode::Chapters);
    }

    controls.side_panel_snapshot.replace(snapshot.clone());
    let interval_times = snapshot_interval_chapters(&snapshot)
        .into_iter()
        .map(|chapter| chapter.time)
        .collect::<Vec<_>>();
    update_timeline_marks(
        &controls.timeline_rail,
        &snapshot.chapters,
        &interval_times,
        &snapshot.bookmarks,
        snapshot.ab_loop,
        media_duration,
    );

    let current_index = snapshot.playlist.iter().position(|item| {
        item.is_current(
            snapshot.current_file.as_deref(),
            snapshot.current_url.as_deref(),
        )
    });

    let mode = controls.side_panel_mode.get();
    update_side_panel_tab_state(&controls.chapters_tab, &controls.up_next_tab, mode);
    clear_list_box(&controls.up_next_list);
    let mut actions = Vec::new();

    match mode {
        SidePanelMode::Chapters => render_chapters_panel(controls, state, &snapshot, &mut actions),
        SidePanelMode::UpNext => {
            render_playlist_panel(controls, state, &snapshot, current_index, &mut actions)
        }
    }

    controls.side_panel_actions.replace(actions);
}

pub(crate) fn render_chapters_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    if !snapshot.chapters.is_empty() {
        // Embedded metadata remains the authoritative, read-only chapter spine.
        controls.up_next_list.append(&panel_heading_row(&format!(
            "CHAPTERS · {}",
            snapshot.chapters.len()
        )));
        actions.push(SidePanelAction::None);

        for (index, chapter) in snapshot.chapters.iter().enumerate() {
            let thumbnail = snapshot
                .current_file
                .as_ref()
                .and_then(|path| thumbnails::existing_thumbnail_path(path, chapter));
            let is_current = snapshot.current_chapter == Some(index);
            controls
                .up_next_list
                .append(&chapter_row(chapter, thumbnail, is_current));
            actions.push(SidePanelAction::Chapter(chapter.time));
        }
    } else {
        // Keep the explicit detection action above the interval list so it remains visible at
        // the initial scroll position even when the fallback reaches its marker cap.
        render_detect_chapters_section(controls, snapshot, actions);
        render_interval_section(controls, snapshot, actions);
    }

    // Bookmarks live alongside the file's own chapters but stay in their own section so
    // the read-only chapter spine (navigation, thumbnails, current-state) is untouched.
    // Only a local file can carry persistent bookmarks, so a stream shows no section.
    if snapshot.current_file.is_some() {
        render_bookmarks_section(controls, state, snapshot, actions);
    }
}

pub(crate) fn snapshot_has_chapter_surface(snapshot: &SidePanelSnapshot) -> bool {
    chapter_math::active_chapter_source(
        !snapshot.chapters.is_empty(),
        snapshot.duration.unwrap_or(0.0),
    )
    .is_some()
}

/// Interval markers for metadata-less media. Embedded chapters always win and are never
/// mixed with synthesized fallback markers.
pub(crate) fn snapshot_interval_chapters(
    snapshot: &SidePanelSnapshot,
) -> Vec<chapter_math::IntervalChapter> {
    if matches!(
        chapter_math::active_chapter_source(
            !snapshot.chapters.is_empty(),
            snapshot.duration.unwrap_or(0.0),
        ),
        Some(chapter_math::ChapterSource::Interval)
    ) {
        chapter_math::fallback_interval_chapters(snapshot.duration.unwrap_or(0.0))
    } else {
        Vec::new()
    }
}

pub(crate) fn render_interval_section(
    controls: &Controls,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    let intervals = snapshot_interval_chapters(snapshot);
    if intervals.is_empty() {
        controls
            .up_next_list
            .append(&panel_empty_row("No chapters in this media yet."));
        actions.push(SidePanelAction::None);
        return;
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "INTERVAL MARKERS · {}",
        intervals.len()
    )));
    actions.push(SidePanelAction::None);
    controls.up_next_list.append(&panel_caption_row(
        "Evenly spaced preview points for media without chapter metadata.",
    ));
    actions.push(SidePanelAction::None);

    for interval in intervals {
        controls
            .up_next_list
            .append(&interval_row(interval.index, interval.time));
        actions.push(SidePanelAction::Chapter(interval.time));
    }
}

pub(crate) fn render_detect_chapters_section(
    controls: &Controls,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    controls
        .up_next_list
        .append(&panel_heading_row("SCENE DETECTION"));
    actions.push(SidePanelAction::None);

    match snapshot.detection {
        chapter_math::ChapterDetection::Idle => {
            controls.up_next_list.append(&detect_chapters_row());
            actions.push(SidePanelAction::DetectChapters);
        }
        chapter_math::ChapterDetection::Detecting { percent } => {
            controls.up_next_list.append(&detection_status_row(&format!(
                "Detecting chapters… {percent}%"
            )));
            actions.push(SidePanelAction::None);
        }
        chapter_math::ChapterDetection::Done { count } => {
            controls
                .up_next_list
                .append(&detection_status_row(&format!("Detected {count} chapters")));
            actions.push(SidePanelAction::None);
        }
        chapter_math::ChapterDetection::Unavailable => {
            controls
                .up_next_list
                .append(&detection_status_row(SCENE_DETECTION_UNAVAILABLE_MESSAGE));
            actions.push(SidePanelAction::None);
        }
    }
}

/// The user's own position bookmarks: a heading, the always-present "add at the current
/// position" affordance, then one row per saved mark (tap to jump, trash to remove).
pub(crate) fn render_bookmarks_section(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    controls.up_next_list.append(&panel_heading_row(&format!(
        "BOOKMARKS · {}",
        snapshot.bookmarks.len()
    )));
    actions.push(SidePanelAction::None);

    for &time in &snapshot.bookmarks {
        controls.up_next_list.append(&bookmark_row(
            time,
            Rc::clone(state),
            Rc::clone(&controls.status_toast),
        ));
        // A bookmark row seeks on activation, exactly like a chapter row.
        actions.push(SidePanelAction::Chapter(time));
    }

    controls.up_next_list.append(&add_bookmark_row());
    actions.push(SidePanelAction::AddBookmark);
}

/// The "Add bookmark at current position" affordance row. Activation is dispatched
/// through [`SidePanelAction::AddBookmark`] by the list's row-activated handler, so this
/// only needs to render — keeping it a plain (state-free) widget the poll can rebuild.
pub(crate) fn add_bookmark_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-add-bookmark-row");
    row.set_selectable(false);
    row.set_tooltip_text(Some("Save a bookmark at the current position"));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("list-add-symbolic");
    icon.add_css_class("okp-add-bookmark-icon");
    icon.set_pixel_size(16);
    icon.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(Some("Add bookmark at current position"));
    label.add_css_class("okp-up-next-file");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);

    row_box.append(&icon);
    row_box.append(&label);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn bookmark_row(
    time: f64,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-bookmark-row");
    row.set_selectable(false);
    row.set_tooltip_text(Some("Jump to bookmark"));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("user-bookmarks-symbolic");
    icon.add_css_class("okp-bookmark-icon");
    icon.set_pixel_size(15);
    icon.set_valign(gtk::Align::Center);

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);
    label_box.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some("Bookmark"));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);

    let marker = gtk::Label::new(Some(&time_code::format_clock(time)));
    marker.add_css_class("okp-up-next-marker");
    marker.set_xalign(0.0);

    label_box.append(&title);
    label_box.append(&marker);

    let remove = playlist_action_button("list-remove-symbolic", "Remove bookmark", true);
    remove.connect_clicked(move |_| {
        remove_bookmark_at(&state, &status_toast, time);
    });

    row_box.append(&icon);
    row_box.append(&label_box);
    row_box.append(&remove);
    row.set_child(Some(&row_box));
    row
}

/// Index of the chapter the playhead sits in, resolved through the shared core
/// [`chapter_math::current_index`] logic (last start at/before `position`, within
/// its default epsilon). `None` before the first chapter or without a position.
pub(crate) fn current_chapter_index(chapters: &[Chapter], position: Option<f64>) -> Option<usize> {
    let position = position.filter(|value| value.is_finite())?;
    let times = chapters
        .iter()
        .map(|chapter| chapter.time)
        .collect::<Vec<_>>();
    chapter_math::current_index(&times, position, chapter_math::DEFAULT_EPSILON)
}

pub(crate) fn render_playlist_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
    current_index: Option<usize>,
    actions: &mut Vec<SidePanelAction>,
) {
    if snapshot.playlist.len() <= 1 {
        render_short_queue_panel(controls, snapshot, current_index, actions);
        return;
    }

    if let Some(current) = current_index.and_then(|index| snapshot.playlist.get(index)) {
        controls.up_next_list.append(&now_playing_pinned_row(
            current,
            true,
            snapshot.current_title.as_deref(),
        ));
        actions.push(SidePanelAction::None);
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "FROM THIS FOLDER · {}",
        snapshot.playlist.len()
    )));
    actions.push(SidePanelAction::None);

    for (index, item) in snapshot.playlist.iter().enumerate() {
        controls.up_next_list.append(&playlist_row(
            item,
            index,
            current_index,
            snapshot.current_title.as_deref(),
            snapshot.playlist.len(),
            Rc::clone(state),
        ));
        actions.push(SidePanelAction::Playlist(index));
    }
}

/// The Up Next panel for a queue too short to scroll (PRD §2.6: a single URL / no
/// folder session shows the now-playing item + an "Add files…" hint). The old
/// branch rendered a bare "No folder queue for this media yet." dead string and
/// dropped the now-playing item entirely, so the panel read as blank with no
/// way back to growing a queue. The lone current item is pinned as a clean
/// now-playing row (no reorder/remove controls — there is nothing to reorder or
/// remove), then the dashed "Add files to queue" affordance opens the same
/// multi-select media dialog the overflow menu's "Add to Queue" uses.
pub(crate) fn render_short_queue_panel(
    controls: &Controls,
    snapshot: &SidePanelSnapshot,
    current_index: Option<usize>,
    actions: &mut Vec<SidePanelAction>,
) {
    if let Some(item) = snapshot.playlist.first() {
        controls.up_next_list.append(&now_playing_pinned_row(
            item,
            current_index == Some(0),
            snapshot.current_title.as_deref(),
        ));
        // The pinned now-playing row is non-activatable; reserve a slot so the
        // Add files affordance below keeps a stable action index.
        actions.push(SidePanelAction::None);
    }

    controls.up_next_list.append(&add_files_row());
    actions.push(SidePanelAction::AddFiles);
}

/// The pinned now-playing row used at the top of a short queue. It mirrors the
/// full queue's current row (NOW badge + source icon + ellipsizing title) but
/// drops the reorder / remove action buttons — a single-item queue has nothing
/// to reorder and the current item is not removable — so the lone entry reads as
/// a calm pinned card instead of a row of greyed-out controls.
pub(crate) fn now_playing_pinned_row(
    item: &PlaylistItem,
    is_current: bool,
    current_title: Option<&str>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-now-playing-pinned-row");
    row.set_activatable(false);
    row.set_selectable(false);
    if is_current {
        row.add_css_class("is-current");
    }
    row.set_tooltip_text(Some(&item.display_location()));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let thumbnail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    thumbnail.add_css_class("okp-now-playing-thumb");
    thumbnail.set_size_request(54, 34);
    thumbnail.set_valign(gtk::Align::Center);
    let icon = playlist_row_icon(item);
    icon.set_pixel_size(14);
    icon.set_hexpand(true);
    icon.set_vexpand(true);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    thumbnail.append(&icon);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 1);
    labels.set_hexpand(true);
    labels.set_valign(gtk::Align::Center);
    let display_title = current_title
        .map(str::to_owned)
        .unwrap_or_else(|| item.display_name());
    let title = gtk::Label::new(Some(&display_title));
    title.add_css_class("okp-now-playing-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);
    let state = gtk::Label::new(Some("NOW PLAYING"));
    state.add_css_class("okp-now-playing-state");
    state.set_xalign(0.0);
    labels.append(&title);
    labels.append(&state);

    row_box.append(&thumbnail);
    row_box.append(&labels);
    row.set_child(Some(&row_box));
    row
}

/// The "Add files to queue" affordance row for the short-queue state. Activation
/// is dispatched through [`SidePanelAction::AddFiles`] by the list's row-activated
/// handler, so this only needs to render — keeping it a plain (state-free) widget
/// the poll can rebuild, mirroring [`add_bookmark_row`].
pub(crate) fn add_files_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-add-files-row");
    row.set_selectable(false);
    row.set_tooltip_text(Some("Append local media files to Up Next"));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("list-add-symbolic");
    icon.add_css_class("okp-add-files-icon");
    icon.set_pixel_size(16);
    icon.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(Some("Add files to queue"));
    label.add_css_class("okp-up-next-file");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);

    row_box.append(&icon);
    row_box.append(&label);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn drain_thumbnail_events(controls: &Controls, state: &Rc<RefCell<PlayerState>>) {
    let mut changed = false;
    while let Ok(event) = controls.thumbnail_events.borrow().try_recv() {
        match event {
            thumbnails::ThumbnailEvent::ChapterReady => {
                changed = true;
            }
            thumbnails::ThumbnailEvent::HoverReady { request_key, path } => {
                clear_hover_thumbnail_request(state, &request_key);
                controls
                    .seek_hover_preview
                    .show_thumbnail_if_current(&request_key, &path);
            }
            thumbnails::ThumbnailEvent::HoverFailed { request_key } => {
                clear_hover_thumbnail_request(state, &request_key);
            }
        }
    }

    if changed {
        controls
            .side_panel_snapshot
            .replace(SidePanelSnapshot::default());
    }
}

fn clear_hover_thumbnail_request(state: &Rc<RefCell<PlayerState>>, request_key: &str) {
    let mut state = state.borrow_mut();
    if state.hover_thumbnail_request_key.as_deref() == Some(request_key) {
        state.hover_thumbnail_request_key = None;
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
            controls.thumbnail_sender.clone(),
        );
    }
}

pub(crate) fn update_timeline_marks(
    rail: &TimelineRail,
    chapters: &[Chapter],
    intervals: &[f64],
    bookmarks: &[f64],
    ab_loop: AbLoopState,
    duration: f64,
) {
    let marks = timeline_marks(chapters, intervals, bookmarks, ab_loop, duration);
    rail.set_marks(marks);
}

pub(crate) fn timeline_marks(
    chapters: &[Chapter],
    intervals: &[f64],
    bookmarks: &[f64],
    ab_loop: AbLoopState,
    duration: f64,
) -> Vec<TimelineMark> {
    let mut marks = chapters
        .iter()
        .map(|chapter| TimelineMark {
            time: chapter.time,
            kind: TimelineMarkKind::Chapter,
        })
        .filter(|mark| timeline_mark_in_range(mark.time, duration))
        .collect::<Vec<_>>();

    marks.extend(
        intervals
            .iter()
            .copied()
            .filter(|time| timeline_mark_in_range(*time, duration))
            .map(|time| TimelineMark {
                time,
                kind: TimelineMarkKind::Interval,
            }),
    );

    marks.extend(
        bookmarks
            .iter()
            .copied()
            .filter(|time| timeline_mark_in_range(*time, duration))
            .map(|time| TimelineMark {
                time,
                kind: TimelineMarkKind::Bookmark,
            }),
    );

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

/// Whether a chapter or bookmark tick belongs on the timeline: it must be finite and
/// sit strictly inside the media, so a mark on the very start handle (`<= 0.0`) or at
/// or past the end (`>= duration`) is dropped rather than piling onto a handle. A
/// non-positive `duration` means the length is not known yet, so the upper bound is
/// skipped and the mark is kept until the real duration arrives.
fn timeline_mark_in_range(time: f64, duration: f64) -> bool {
    time.is_finite() && time > 0.0 && (duration <= 0.0 || time < duration)
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

pub(crate) fn panel_caption_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-caption-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-caption");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);
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
    label.set_justify(gtk::Justification::Center);
    label.set_xalign(0.5);
    row.set_child(Some(&label));
    row
}

pub(crate) fn interval_row(index: usize, time: f64) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-interval-row");
    row.set_selectable(false);
    row.set_tooltip_text(Some("Jump to this interval marker"));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("media-seek-forward-symbolic");
    icon.add_css_class("okp-interval-icon");
    icon.set_pixel_size(15);
    icon.set_valign(gtk::Align::Center);

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);
    label_box.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some(&format!(
        "{} {}",
        chapter_math::ChapterSource::Interval.label(),
        index + 1
    )));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);

    let marker = gtk::Label::new(Some(&time_code::format_clock(time)));
    marker.add_css_class("okp-up-next-marker");
    marker.set_xalign(0.0);

    label_box.append(&title);
    label_box.append(&marker);
    row_box.append(&icon);
    row_box.append(&label_box);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn detect_chapters_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-detect-row");
    row.set_selectable(false);
    row.set_tooltip_text(Some("Scan the video for scene changes"));

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("system-search-symbolic");
    icon.add_css_class("okp-detect-icon");
    icon.set_pixel_size(16);
    icon.set_valign(gtk::Align::Center);

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);
    label_box.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some("Detect chapters"));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);

    let subtitle = gtk::Label::new(Some("Scan for scene changes in the background"));
    subtitle.add_css_class("okp-detect-subtitle");
    subtitle.set_xalign(0.0);

    label_box.append(&title);
    label_box.append(&subtitle);
    row_box.append(&icon);
    row_box.append(&label_box);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn detection_status_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-detect-status-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.set_hexpand(true);

    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.add_css_class("okp-detect-status-icon");
    icon.set_pixel_size(14);
    icon.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-detect-status");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);

    row_box.append(&icon);
    row_box.append(&label);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn chapter_row(
    chapter: &Chapter,
    thumbnail: Option<PathBuf>,
    is_current: bool,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.add_css_class("okp-chapter-row");
    row.set_selectable(false);
    if is_current {
        row.add_css_class("is-current");
    }

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let current_rail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    current_rail.add_css_class("okp-chapter-current-rail");
    current_rail.set_size_request(3, 34);
    current_rail.set_valign(gtk::Align::Center);
    if is_current {
        current_rail.add_css_class("is-current");
    }

    let thumbnail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    thumbnail_box.add_css_class("okp-chapter-thumb");
    thumbnail_box.set_size_request(56, 32);
    thumbnail_box.set_valign(gtk::Align::Center);
    if let Some(thumbnail) = thumbnail {
        let picture = gtk::Picture::for_filename(thumbnail);
        picture.set_size_request(56, 32);
        picture.set_can_shrink(true);
        thumbnail_box.append(&picture);
    } else {
        // No frame is cached yet (thumbnails warm asynchronously), so show a calm
        // placeholder glyph rather than a bare grey rectangle — the row reads as
        // "preview pending" instead of broken.
        thumbnail_box.add_css_class("is-pending");
        let placeholder = gtk::Image::from_icon_name("image-x-generic-symbolic");
        placeholder.add_css_class("okp-chapter-thumb-placeholder");
        placeholder.set_pixel_size(18);
        placeholder.set_hexpand(true);
        placeholder.set_vexpand(true);
        placeholder.set_halign(gtk::Align::Center);
        placeholder.set_valign(gtk::Align::Center);
        thumbnail_box.append(&placeholder);
    }

    let title_text = chapter
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));

    let title = gtk::Label::new(Some(&title_text));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    let time = gtk::Label::new(Some(&time_code::format_clock(chapter.time)));
    time.add_css_class("okp-up-next-marker");
    time.set_xalign(1.0);
    time.set_valign(gtk::Align::Center);

    row_box.append(&current_rail);
    row_box.append(&thumbnail_box);
    row_box.append(&title);
    row_box.append(&time);
    row.set_child(Some(&row_box));
    row
}

/// A small accent pill used to flag the now-playing chapter or queue item.
pub(crate) fn now_playing_badge(text: &str) -> gtk::Label {
    let badge = gtk::Label::new(Some(text));
    badge.add_css_class("okp-now-badge");
    badge.set_valign(gtk::Align::Center);
    badge
}

/// A quieter pill used to flag the item that plays next in the queue.
pub(crate) fn up_next_badge(text: &str) -> gtk::Label {
    let badge = gtk::Label::new(Some(text));
    badge.add_css_class("okp-next-badge");
    badge.set_valign(gtk::Align::Center);
    badge
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
    current_title: Option<&str>,
    playlist_len: usize,
    state: Rc<RefCell<PlayerState>>,
) -> gtk::ListBoxRow {
    let is_current = current_index == Some(index);
    let is_next = current_index.is_some_and(|current| index == current + 1);
    let is_behind = current_index.is_some_and(|current| index < current);
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_activatable(!is_current);
    row.set_selectable(false);
    row.set_tooltip_text(Some(&item.display_location()));
    if is_current {
        row.add_css_class("is-current");
    } else if is_behind {
        // Already played this session — dim it so the eye lands on what is next.
        row.add_css_class("is-behind");
    }

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    // A fixed-width lane keeps the now/next badge or queue number in a single
    // column so the titles line up down the list.
    let lane = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    lane.add_css_class("okp-up-next-lane");
    lane.set_halign(gtk::Align::Start);
    lane.set_valign(gtk::Align::Center);
    if is_current {
        lane.append(&now_playing_badge("NOW"));
    } else if is_next {
        lane.append(&up_next_badge("NEXT"));
    } else if is_behind {
        let watched = gtk::Image::from_icon_name("object-select-symbolic");
        watched.add_css_class("okp-up-next-watched-icon");
        watched.set_pixel_size(13);
        watched.set_hexpand(true);
        watched.set_halign(gtk::Align::Center);
        lane.append(&watched);
    } else {
        let number = gtk::Label::new(Some(&format!("{}", index + 1)));
        number.add_css_class("okp-up-next-index");
        number.set_xalign(0.5);
        number.set_hexpand(true);
        lane.append(&number);
    }

    let icon = playlist_row_icon(item);

    let display_title = if is_current {
        current_title
            .map(str::to_owned)
            .unwrap_or_else(|| item.display_name())
    } else {
        item.display_name()
    };
    let title = gtk::Label::new(Some(&display_title));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    let drag_handle = playlist_drag_handle();

    let actions = playlist_actions_menu(
        index,
        current_index,
        playlist_len,
        is_current,
        Rc::clone(&state),
    );

    connect_playlist_row_drag_reorder(&row, &drag_handle, index, state);

    row_box.append(&drag_handle);
    row_box.append(&lane);
    row_box.append(&icon);
    row_box.append(&title);
    row_box.append(&actions);
    row.set_child(Some(&row_box));
    row
}

pub(crate) fn playlist_actions_menu(
    index: usize,
    current_index: Option<usize>,
    playlist_len: usize,
    is_current: bool,
    state: Rc<RefCell<PlayerState>>,
) -> gtk::MenuButton {
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build();
    menu.add_css_class("okp-up-next-actions-menu");
    menu.set_has_frame(false);
    menu.set_tooltip_text(Some("Queue actions"));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.add_css_class("okp-up-next-actions-popover");

    let move_up = playlist_menu_action("go-up-symbolic", "Move up", index > 0);
    let move_up_state = Rc::clone(&state);
    move_up.connect_clicked(move |_| {
        move_playlist_item(&move_up_state, index, index.saturating_sub(1));
    });
    content.append(&move_up);

    let play_next_sensitive =
        current_index.is_some_and(|current| index != current && index != current + 1);
    let play_next = playlist_menu_action(
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
    content.append(&play_next);

    let move_down = playlist_menu_action("go-down-symbolic", "Move down", index + 1 < playlist_len);
    let move_down_state = Rc::clone(&state);
    move_down.connect_clicked(move |_| {
        move_playlist_item(&move_down_state, index, index + 1);
    });
    content.append(&move_down);

    let remove = playlist_menu_action(
        "list-remove-symbolic",
        "Remove from queue",
        !is_current && playlist_len > 1,
    );
    let remove_state = Rc::clone(&state);
    remove.connect_clicked(move |_| {
        remove_playlist_item(&remove_state, index);
    });
    content.append(&remove);

    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_child(Some(&content));
    menu.set_popover(Some(&popover));
    menu
}

pub(crate) fn playlist_menu_action(icon_name: &str, label: &str, sensitive: bool) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .label(label)
        .build();
    button.add_css_class("okp-up-next-menu-action");
    button.set_has_frame(false);
    button.set_sensitive(sensitive);
    button
}

/// A leading glyph that tells a local file apart from a stream URL, so the
/// queue reads its source at a glance instead of only from the file name.
pub(crate) fn playlist_row_icon(item: &PlaylistItem) -> gtk::Image {
    let icon_name = match item {
        PlaylistItem::Local(_) => "video-x-generic-symbolic",
        PlaylistItem::Url(_) => "network-server-symbolic",
    };
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class("okp-up-next-source-icon");
    icon.set_pixel_size(14);
    icon.set_valign(gtk::Align::Center);
    icon
}

pub(crate) fn playlist_drag_handle() -> gtk::Box {
    let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    handle.add_css_class("okp-up-next-drag-handle");
    handle.set_tooltip_text(Some("Drag to reorder"));
    handle.set_valign(gtk::Align::Center);
    handle.set_can_target(true);

    let icon = gtk::Image::from_icon_name("list-drag-handle-symbolic");
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

/// Representative Chapters/Up Next content used by the visual smoke hook
/// (`OKP_OPEN_SIDE_PANEL_ON_STARTUP`). Fixture data only — the live panel always
/// renders from `Mpv::observed_chapters` and the loaded playlist. It exercises the
/// current-chapter state, a long chapter name that must ellipsize, and a queue
/// whose current item sits in the middle so the played-behind, now-playing, and
/// next rows — plus a mix of local files and a stream URL — are all covered.
pub(crate) fn side_panel_preview_sample() -> SidePanelSnapshot {
    let chapter = |index: i64, time: f64, title: &str| Chapter {
        index,
        time,
        title: Some(title.to_owned()),
    };
    let chapters = vec![
        chapter(0, 0.0, "Cold Open"),
        chapter(1, 312.0, "Main Titles"),
        chapter(
            2,
            933.0,
            "The Long Walk Home — a chapter title long enough to prove it ellipsizes",
        ),
        chapter(3, 1780.0, "The Confrontation"),
        chapter(4, 2540.0, "Resolution & End Credits"),
    ];
    let current_file = PathBuf::from("/media/films/Bonus — Behind the Scenes Featurette.mkv");
    let playlist = vec![
        PlaylistItem::Local(PathBuf::from(
            "/media/films/Feature Presentation (2024).mkv",
        )),
        PlaylistItem::Local(current_file.clone()),
        PlaylistItem::Url("https://stream.example.com/live/channel-one.m3u8".to_owned()),
        PlaylistItem::Local(PathBuf::from(
            "/media/films/Closing Short — A Director's Note.mkv",
        )),
    ];

    SidePanelSnapshot {
        has_media: true,
        current_file: Some(current_file),
        current_url: None,
        current_title: None,
        playlist,
        chapters,
        current_chapter: Some(2),
        duration: Some(2705.0),
        // A couple of user bookmarks so the Bookmarks section (heading, the add row and
        // saved marks) is exercised by the visual smoke shot.
        bookmarks: vec![468.0, 1180.0],
        ab_loop: AbLoopState::default(),
        detection: chapter_math::ChapterDetection::default(),
    }
}

pub(crate) fn side_panel_bookmarks_sample() -> SidePanelSnapshot {
    let mut sample = side_panel_preview_sample();
    sample.chapters.truncate(2);
    sample.current_chapter = Some(1);
    sample.bookmarks = vec![468.0, 1180.0, 2112.0];
    sample
}

pub(crate) fn side_panel_empty_chapters_sample() -> SidePanelSnapshot {
    let current_file = PathBuf::from("/media/films/Unchaptered Feature.mkv");
    SidePanelSnapshot {
        has_media: true,
        current_file: Some(current_file.clone()),
        current_url: None,
        current_title: None,
        playlist: vec![PlaylistItem::Local(current_file)],
        chapters: Vec::new(),
        current_chapter: None,
        duration: None,
        bookmarks: Vec::new(),
        ab_loop: AbLoopState::default(),
        detection: chapter_math::ChapterDetection::default(),
    }
}

/// Representative Up Next content for the short-queue / single-URL state used by
/// the visual smoke hook (`OKP_OPEN_SIDE_PANEL_ON_STARTUP=up-next-empty`). Fixture
/// data only — the live panel always renders from `Mpv::observed_chapters` and
/// the loaded playlist. It exercises the PRD §2.6 "Empty (single URL / no folder)"
/// state: one now-playing stream URL, no chapters, no bookmarks — the surface
/// that used to render a bare dead "No folder queue" string and now pins the
/// now-playing item plus the "Add files to queue" affordance.
pub(crate) fn side_panel_empty_up_next_sample() -> SidePanelSnapshot {
    let url = "https://stream.example.com/live/channel-one.m3u8".to_owned();
    SidePanelSnapshot {
        has_media: true,
        current_file: None,
        current_url: Some(url.clone()),
        current_title: None,
        playlist: vec![PlaylistItem::Url(url)],
        chapters: Vec::new(),
        current_chapter: None,
        duration: None,
        bookmarks: Vec::new(),
        ab_loop: AbLoopState::default(),
        detection: chapter_math::ChapterDetection::default(),
    }
}

/// Metadata-less local media with a known duration, used by the interval-fallback visual
/// smoke state. A bookmark is included so the three source types remain visibly separate.
pub(crate) fn side_panel_interval_preview_sample() -> SidePanelSnapshot {
    let current_file = PathBuf::from("/media/home-video/Cabin Weekend.mp4");
    SidePanelSnapshot {
        has_media: true,
        current_file: Some(current_file.clone()),
        current_url: None,
        current_title: None,
        playlist: vec![PlaylistItem::Local(current_file)],
        chapters: Vec::new(),
        current_chapter: None,
        duration: Some(3600.0),
        bookmarks: vec![742.0],
        ab_loop: AbLoopState::default(),
        detection: chapter_math::ChapterDetection::default(),
    }
}

/// Freeze the live poll and render the side panel from fixture data so the
/// Chapters and Up Next surfaces can be screenshot-tested without loaded media.
/// The freeze lasts only until real media loads: [`update_up_next_panel`]
/// releases it and rebuilds live rows, so an inherited env var can never pin
/// fixtures over a real session. Presentational smoke hook only; production
/// code never calls this.
pub(crate) fn open_side_panel_preview(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    chrome: &ChromeVisibility,
    mode: SidePanelMode,
    snapshot: SidePanelSnapshot,
) {
    controls.side_panel_preview_frozen.set(true);

    controls.side_panel_mode.set(mode);
    set_side_panel_user_visible(
        &controls.up_next_revealer,
        &controls.side_panel_fade_revealer,
        &controls.chapters_button,
        &controls.side_panel_user_visible,
        &controls.side_panel_pinned,
        chrome,
        true,
    );

    update_side_panel_tab_state(&controls.chapters_tab, &controls.up_next_tab, mode);

    clear_list_box(&controls.up_next_list);
    let mut actions = Vec::new();
    match mode {
        SidePanelMode::Chapters => render_chapters_panel(controls, state, &snapshot, &mut actions),
        SidePanelMode::UpNext => {
            let current_index = snapshot.playlist.iter().position(|item| {
                item.is_current(
                    snapshot.current_file.as_deref(),
                    snapshot.current_url.as_deref(),
                )
            });
            render_playlist_panel(controls, state, &snapshot, current_index, &mut actions);
        }
    }
    controls.side_panel_actions.replace(actions);
    let duration = snapshot.duration.unwrap_or(0.0);
    if duration > 0.0 {
        controls.seek.set_range(0.0, duration);
    }
    let intervals = snapshot_interval_chapters(&snapshot)
        .into_iter()
        .map(|chapter| chapter.time)
        .collect::<Vec<_>>();
    update_timeline_marks(
        &controls.timeline_rail,
        &snapshot.chapters,
        &intervals,
        &snapshot.bookmarks,
        snapshot.ab_loop,
        duration,
    );
    controls.side_panel_snapshot.replace(snapshot);
}
