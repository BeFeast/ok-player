use super::*;

pub(crate) fn build_controls(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) -> Controls {
    let play_button = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .build();
    play_button.set_has_frame(false);
    play_button.add_css_class("okp-control-button");
    play_button.add_css_class("okp-play-button");
    play_button.set_tooltip_text(Some("Play / Pause (Space)"));
    play_button.set_sensitive(false);

    let open_button = gtk::Button::with_label("Open");
    open_button.set_has_frame(false);
    open_button.add_css_class("okp-control-button");
    open_button.add_css_class("okp-chip-button");
    open_button.set_tooltip_text(Some("Open file (O)"));

    let subtitle_button = gtk::MenuButton::builder().label("Sub").build();
    subtitle_button.set_has_frame(false);
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.add_css_class("okp-chip-button");
    subtitle_button.set_tooltip_text(Some("Subtitles"));
    subtitle_button.set_sensitive(false);

    let audio_button = gtk::MenuButton::builder().label("Audio").build();
    audio_button.set_has_frame(false);
    audio_button.add_css_class("okp-control-button");
    audio_button.add_css_class("okp-chip-button");
    audio_button.set_tooltip_text(Some("Audio"));
    audio_button.set_sensitive(false);

    let speed_button = gtk::MenuButton::builder().label("1.00x").build();
    speed_button.set_has_frame(false);
    speed_button.add_css_class("okp-control-button");
    speed_button.add_css_class("okp-speed-chip");
    speed_button.set_tooltip_text(Some("Playback speed"));
    speed_button.set_sensitive(false);

    let previous_button = gtk::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .build();
    previous_button.set_has_frame(false);
    previous_button.add_css_class("okp-control-button");
    previous_button.add_css_class("okp-transport-button");
    previous_button.set_tooltip_text(Some("Previous item (Page Up)"));
    previous_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");

    let next_button = gtk::Button::builder()
        .icon_name("media-skip-forward-symbolic")
        .build();
    next_button.set_has_frame(false);
    next_button.add_css_class("okp-control-button");
    next_button.add_css_class("okp-transport-button");
    next_button.set_tooltip_text(Some("Next item (Page Down)"));
    next_button.set_sensitive(false);

    let chapters_button = gtk::Button::builder()
        .icon_name("view-list-symbolic")
        .build();
    chapters_button.set_has_frame(false);
    chapters_button.add_css_class("okp-control-button");
    chapters_button.add_css_class("okp-icon-button");
    chapters_button.set_tooltip_text(Some("Chapters / Up Next"));
    chapters_button.set_sensitive(false);

    let screenshot_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .build();
    screenshot_button.set_has_frame(false);
    screenshot_button.add_css_class("okp-control-button");
    screenshot_button.add_css_class("okp-icon-button");
    screenshot_button.set_tooltip_text(Some("Save frame to Pictures/OK Player (C)"));
    screenshot_button.set_sensitive(false);

    let fullscreen_button = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .build();
    fullscreen_button.set_has_frame(false);
    fullscreen_button.add_css_class("okp-control-button");
    fullscreen_button.add_css_class("okp-icon-button");
    fullscreen_button.set_tooltip_text(Some("Enter Fullscreen (F)"));
    fullscreen_button.set_sensitive(false);

    let more_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build();
    more_button.set_has_frame(false);
    more_button.add_css_class("okp-control-button");
    more_button.add_css_class("okp-icon-button");
    more_button.set_tooltip_text(Some("More commands"));

    let duration_label = gtk::Label::new(Some("00:00"));
    duration_label.add_css_class("okp-time-label");

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    seek.set_sensitive(false);
    seek.add_css_class("okp-seek");

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    volume.set_draw_value(false);
    volume.set_width_request(68);
    volume.set_value(100.0);
    volume.add_css_class("okp-volume");

    let chapters_tab = side_panel_segment_button("Chapters", true);
    let up_next_tab = side_panel_segment_button("Up Next", false);
    let side_panel_mode = Rc::new(Cell::new(SidePanelMode::Chapters));
    let side_panel_tabs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    side_panel_tabs.add_css_class("okp-side-panel-tabs");
    side_panel_tabs.set_halign(gtk::Align::Start);
    side_panel_tabs.set_hexpand(true);
    side_panel_tabs.append(&chapters_tab);
    side_panel_tabs.append(&up_next_tab);

    let side_panel_close = gtk::Button::from_icon_name("window-close-symbolic");
    side_panel_close.add_css_class("okp-side-panel-close");
    side_panel_close.set_has_frame(false);
    side_panel_close.set_tooltip_text(Some("Close panel"));

    let up_next_list = gtk::ListBox::new();
    up_next_list.add_css_class("okp-up-next-list");
    up_next_list.set_selection_mode(gtk::SelectionMode::None);

    let up_next_scroller = gtk::ScrolledWindow::new();
    up_next_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    up_next_scroller.set_child(Some(&up_next_list));
    up_next_scroller.set_vexpand(true);

    let up_next_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    up_next_header.add_css_class("okp-side-panel-header");
    up_next_header.append(&side_panel_tabs);
    up_next_header.append(&side_panel_close);

    let up_next_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    up_next_panel.add_css_class("okp-up-next-panel");
    up_next_panel.set_width_request(SIDE_PANEL_WIDTH);
    up_next_panel.append(&up_next_header);
    up_next_panel.append(&up_next_scroller);

    let side_panel_fade_revealer = gtk::Revealer::new();
    side_panel_fade_revealer.set_transition_duration(SIDE_PANEL_TRANSITION_MS);
    side_panel_fade_revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    side_panel_fade_revealer.set_reveal_child(false);
    side_panel_fade_revealer.set_child(Some(&up_next_panel));

    let up_next_revealer = gtk::Revealer::new();
    up_next_revealer.set_halign(gtk::Align::End);
    up_next_revealer.set_valign(gtk::Align::Fill);
    up_next_revealer.set_margin_top(SIDE_PANEL_TOP_INSET);
    up_next_revealer.set_margin_end(0);
    up_next_revealer.set_margin_bottom(SIDE_PANEL_BOTTOM_INSET);
    up_next_revealer.set_transition_duration(SIDE_PANEL_TRANSITION_MS);
    up_next_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    up_next_revealer.set_reveal_child(false);
    up_next_revealer.set_can_target(false);
    up_next_revealer.set_child(Some(&side_panel_fade_revealer));

    let side_panel_user_visible = Rc::new(Cell::new(false));
    let side_panel_pinned = Rc::new(Cell::new(false));
    let side_panel_manual_mode = Rc::new(Cell::new(false));
    let side_panel_snapshot = Rc::new(RefCell::new(SidePanelSnapshot::default()));

    let close_panel = up_next_revealer.clone();
    let close_fade = side_panel_fade_revealer.clone();
    let close_toggle = chapters_button.clone();
    let close_visible = Rc::clone(&side_panel_user_visible);
    let close_pinned = Rc::clone(&side_panel_pinned);
    let close_chrome = Rc::clone(&chrome);
    side_panel_close.connect_clicked(move |_| {
        set_side_panel_user_visible(
            &close_panel,
            &close_fade,
            &close_toggle,
            &close_visible,
            &close_pinned,
            &close_chrome,
            false,
        );
    });

    let up_next_state = Rc::clone(&state);
    let up_next_actions = Rc::new(RefCell::new(Vec::<SidePanelAction>::new()));
    let row_actions = Rc::clone(&up_next_actions);
    let row_toast = Rc::clone(&status_toast);
    let row_parent = window.clone();
    let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
    up_next_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }

        match row_actions
            .borrow()
            .get(index as usize)
            .copied()
            .unwrap_or(SidePanelAction::None)
        {
            SidePanelAction::None => {}
            SidePanelAction::Chapter(time) => seek_to_chapter(&up_next_state, time),
            SidePanelAction::Playlist(index) => {
                jump_playlist_index(&up_next_state, index);
            }
            SidePanelAction::AddBookmark => add_bookmark_at_position(&up_next_state, &row_toast),
            // The Up Next short-queue state's "Add files" affordance: opens the
            // same multi-select media dialog the overflow menu's "Add to Queue"
            // uses, so a single-URL / no-folder session can grow a queue without
            // leaving the panel (PRD §2.6).
            SidePanelAction::AddFiles => open_queue_media_dialog(
                &row_parent,
                Rc::clone(&up_next_state),
                Rc::clone(&row_toast),
                QueueInsertMode::Append,
            ),
        }
    });

    let chapters_tab_mode = Rc::clone(&side_panel_mode);
    let chapters_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let chapters_tab_button = chapters_tab.clone();
    let chapters_peer_tab = up_next_tab.clone();
    chapters_tab.connect_clicked(move |_| {
        chapters_tab_manual_mode.set(true);
        chapters_tab_mode.set(SidePanelMode::Chapters);
        chapters_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &chapters_tab_button,
            &chapters_peer_tab,
            SidePanelMode::Chapters,
        );
    });

    let up_next_tab_mode = Rc::clone(&side_panel_mode);
    let up_next_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let up_next_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let up_next_tab_button = up_next_tab.clone();
    let up_next_peer_tab = chapters_tab.clone();
    up_next_tab.connect_clicked(move |_| {
        up_next_tab_manual_mode.set(true);
        up_next_tab_mode.set(SidePanelMode::UpNext);
        up_next_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &up_next_peer_tab,
            &up_next_tab_button,
            SidePanelMode::UpNext,
        );
    });

    let subtitle_popover = gtk::Popover::new();
    prepare_track_popover(&subtitle_popover);
    connect_popover_chrome_pin(&subtitle_popover, Rc::clone(&chrome));
    subtitle_button.set_popover(Some(&subtitle_popover));
    let subtitle_parent = window.clone();
    let subtitle_state = Rc::clone(&state);
    subtitle_popover.connect_show(move |popover| {
        populate_subtitle_popover(popover, &subtitle_parent, Rc::clone(&subtitle_state));
    });

    let audio_popover = gtk::Popover::new();
    prepare_track_popover(&audio_popover);
    connect_popover_chrome_pin(&audio_popover, Rc::clone(&chrome));
    audio_button.set_popover(Some(&audio_popover));
    let audio_state = Rc::clone(&state);
    let audio_toast = Rc::clone(&status_toast);
    audio_popover.connect_show(move |popover| {
        populate_audio_popover(popover, Rc::clone(&audio_state), Rc::clone(&audio_toast));
    });

    let speed_popover = gtk::Popover::new();
    prepare_track_popover(&speed_popover);
    connect_popover_chrome_pin(&speed_popover, Rc::clone(&chrome));
    speed_button.set_popover(Some(&speed_popover));
    let speed_state = Rc::clone(&state);
    speed_popover.connect_show(move |popover| {
        populate_speed_popover(popover, Rc::clone(&speed_state));
    });

    let more_popover = gtk::Popover::new();
    prepare_track_popover(&more_popover);
    connect_popover_chrome_pin(&more_popover, Rc::clone(&chrome));
    more_button.set_popover(Some(&more_popover));
    let more_parent = window.clone();
    let more_state = Rc::clone(&state);
    let more_toast = Rc::clone(&status_toast);
    more_popover.connect_show(move |popover| {
        populate_command_popover(
            popover,
            &more_parent,
            Rc::clone(&more_state),
            Rc::clone(&more_toast),
        );
    });

    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    let open_toast = Rc::clone(&status_toast);
    open_button.connect_clicked(move |_| {
        open_media_dialog(&open_parent, Rc::clone(&open_state), Rc::clone(&open_toast))
    });

    let previous_state = Rc::clone(&state);
    previous_button.connect_clicked(move |_| {
        navigate_playlist(&previous_state, -1);
    });

    let play_state = Rc::clone(&state);
    let play_open_parent = window.clone();
    let play_open_toast = Rc::clone(&status_toast);
    play_button.connect_clicked(move |_| {
        let has_media = has_loaded_media(&play_state);
        if !has_media {
            open_media_dialog(
                &play_open_parent,
                Rc::clone(&play_state),
                Rc::clone(&play_open_toast),
            );
            return;
        }

        if let Some(mpv) = play_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.cycle_pause()
        {
            eprintln!("Failed to toggle playback: {error}");
        }
    });

    let next_state = Rc::clone(&state);
    next_button.connect_clicked(move |_| {
        navigate_playlist(&next_state, 1);
    });

    let chapters_panel = up_next_revealer.clone();
    let chapters_fade = side_panel_fade_revealer.clone();
    let chapters_toggle = chapters_button.clone();
    let chapters_visible = Rc::clone(&side_panel_user_visible);
    let chapters_pinned = Rc::clone(&side_panel_pinned);
    let chapters_chrome = Rc::clone(&chrome);
    let chapters_state = Rc::clone(&state);
    let chapters_mode = Rc::clone(&side_panel_mode);
    let chapters_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_for_toggle = chapters_tab.clone();
    let up_next_tab_for_toggle = up_next_tab.clone();
    let chapters_snapshot_for_toggle = Rc::clone(&side_panel_snapshot);
    chapters_button.connect_clicked(move |_| {
        let next_visible = !chapters_visible.get();
        if next_visible {
            let preferred_mode = preferred_side_panel_mode(&chapters_state);
            chapters_manual_mode.set(false);
            chapters_mode.set(preferred_mode);
            chapters_snapshot_for_toggle.borrow_mut().has_media = false;
            update_side_panel_tab_state(
                &chapters_tab_for_toggle,
                &up_next_tab_for_toggle,
                preferred_mode,
            );
        }
        set_side_panel_user_visible(
            &chapters_panel,
            &chapters_fade,
            &chapters_toggle,
            &chapters_visible,
            &chapters_pinned,
            &chapters_chrome,
            next_visible,
        );
    });

    let screenshot_state = Rc::clone(&state);
    let screenshot_toast = Rc::clone(&status_toast);
    screenshot_button
        .connect_clicked(move |_| save_screenshot(&screenshot_state, &screenshot_toast, false));

    let fullscreen_parent = window.clone();
    fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&fullscreen_parent));

    let seek_state = Rc::clone(&state);
    seek.connect_change_value(move |_, _, value| {
        if !updating_seek.get() {
            seek_absolute(&seek_state, value);
        }

        glib::Propagation::Proceed
    });
    let seek_hover_preview = connect_seek_hover(&seek, Rc::clone(&state), thumbnail_sender.clone());

    let volume_state = Rc::clone(&state);
    volume.connect_change_value(move |_, _, value| {
        if !updating_volume.get() {
            set_volume_from_ui(&volume_state, value);
        }

        glib::Propagation::Proceed
    });

    Controls {
        open_button,
        subtitle_button,
        audio_button,
        speed_button,
        previous_button,
        play_button,
        next_button,
        chapters_button,
        screenshot_button,
        fullscreen_button,
        more_button,
        seek,
        elapsed_label,
        duration_label,
        volume,
        status_toast,
        timeline_marks_snapshot: RefCell::new(Vec::new()),
        up_next_revealer,
        side_panel_fade_revealer,
        chapters_tab,
        up_next_tab,
        up_next_list,
        side_panel_user_visible,
        side_panel_pinned,
        side_panel_mode,
        side_panel_manual_mode,
        side_panel_snapshot,
        side_panel_actions: up_next_actions,
        side_panel_preview_frozen: Rc::new(Cell::new(false)),
        seek_hover_preview,
        thumbnail_sender,
        thumbnail_events: RefCell::new(thumbnail_receiver),
    }
}

pub(crate) fn controls_bar(controls: &Controls) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.add_css_class("okp-controls");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_start(14);
    bar.set_margin_end(14);
    bar.set_margin_bottom(14);

    let transport = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    transport.add_css_class("okp-transport-group");
    transport.append(&controls.previous_button);
    transport.append(&controls.play_button);
    transport.append(&controls.next_button);

    let primary = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    primary.add_css_class("okp-command-cluster");
    primary.append(&controls.open_button);
    primary.append(&transport);

    let timeline = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    timeline.add_css_class("okp-timeline-group");
    timeline.set_hexpand(true);
    timeline.append(&controls.elapsed_label);
    timeline.append(&controls.seek);
    timeline.append(&controls.duration_label);

    let secondary = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    secondary.add_css_class("okp-command-cluster");
    secondary.append(&controls.volume);
    secondary.append(&controls.speed_button);
    secondary.append(&controls.subtitle_button);
    secondary.append(&controls.audio_button);
    secondary.append(&controls.chapters_button);
    secondary.append(&controls.screenshot_button);
    secondary.append(&controls.fullscreen_button);
    secondary.append(&controls.more_button);

    let primary_separator = gtk::Separator::new(gtk::Orientation::Vertical);
    primary_separator.add_css_class("okp-control-separator");
    let secondary_separator = gtk::Separator::new(gtk::Orientation::Vertical);
    secondary_separator.add_css_class("okp-control-separator");

    bar.append(&primary);
    bar.append(&primary_separator);
    bar.append(&timeline);
    bar.append(&secondary_separator);
    bar.append(&secondary);

    bar
}

pub(crate) fn connect_chrome_activity(overlay: &gtk::Overlay, chrome: Rc<ChromeVisibility>) {
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        chrome.show_for_activity();
    });
    overlay.add_controller(motion);
}

pub(crate) fn connect_popover_chrome_pin(popover: &gtk::Popover, chrome: Rc<ChromeVisibility>) {
    let show_chrome = Rc::clone(&chrome);
    popover.connect_show(move |_| {
        show_chrome.pin();
    });

    popover.connect_closed(move |_| {
        chrome.unpin();
    });
}

pub(crate) fn prepare_track_popover(popover: &gtk::Popover) {
    popover.add_css_class("okp-track-popover");
    popover.set_has_arrow(false);
}

pub(crate) fn side_panel_segment_button(label: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("okp-side-panel-tab");
    button.set_has_frame(false);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

pub(crate) fn preferred_side_panel_mode(state: &Rc<RefCell<PlayerState>>) -> SidePanelMode {
    let state = state.borrow();
    let has_chapters = state
        .mpv
        .as_ref()
        .map(Mpv::observed_chapters)
        .is_some_and(|chapters| !chapters.is_empty());
    if has_chapters {
        SidePanelMode::Chapters
    } else {
        SidePanelMode::UpNext
    }
}

pub(crate) fn update_side_panel_tab_state(
    chapters_tab: &gtk::Button,
    up_next_tab: &gtk::Button,
    mode: SidePanelMode,
) {
    match mode {
        SidePanelMode::Chapters => {
            chapters_tab.add_css_class("is-selected");
            up_next_tab.remove_css_class("is-selected");
        }
        SidePanelMode::UpNext => {
            up_next_tab.add_css_class("is-selected");
            chapters_tab.remove_css_class("is-selected");
        }
    }
}

pub(crate) fn set_side_panel_user_visible(
    revealer: &gtk::Revealer,
    fade_revealer: &gtk::Revealer,
    toggle: &gtk::Button,
    user_visible: &Rc<Cell<bool>>,
    pinned: &Rc<Cell<bool>>,
    chrome: &ChromeVisibility,
    visible: bool,
) {
    user_visible.set(visible);
    revealer.set_can_target(visible);
    fade_revealer.set_reveal_child(visible);
    revealer.set_reveal_child(visible);

    if visible {
        toggle.add_css_class("is-selected");
        if pinned.get() {
            chrome.show_persistently();
        } else {
            chrome.pin();
            pinned.set(true);
        }
    } else {
        toggle.remove_css_class("is-selected");
        if pinned.replace(false) {
            chrome.unpin();
        }
    }
}

pub(crate) fn update_fullscreen_button(button: &gtk::Button, is_fullscreen: bool) {
    if is_fullscreen {
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Exit Fullscreen (F / Esc)"));
        button.add_css_class("is-selected");
    } else {
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Enter Fullscreen (F)"));
        button.remove_css_class("is-selected");
    }
}

pub(crate) fn connect_seek_hover(
    seek: &gtk::Scale,
    state: Rc<RefCell<PlayerState>>,
    thumbnail_sender: mpsc::Sender<String>,
) -> Rc<SeekHoverPreview> {
    let preview = Rc::new(SeekHoverPreview::new(seek));
    let motion = gtk::EventControllerMotion::new();

    let motion_seek = seek.clone();
    let motion_state = Rc::clone(&state);
    let motion_preview = Rc::clone(&preview);
    motion.connect_motion(move |_, x, _| {
        let Some((media_path, duration, chapters)) = seek_hover_snapshot(&motion_state) else {
            motion_preview.hide();
            return;
        };

        let width = f64::from(motion_seek.width().max(1));
        let time = (x.clamp(0.0, width) / width * duration).clamp(0.0, duration);
        // Only a local file can be sampled for a hover thumbnail; a stream (or any
        // source without a file on disk) still gets the timecode + chapter preview,
        // just with no thumbnail — the deliberate timecode-only fallback.
        let thumbnail = media_path.as_deref().and_then(|path| {
            hover_thumbnail_for_time(&motion_state, path, time, duration, &thumbnail_sender)
        });
        motion_preview.show(
            &motion_seek,
            x,
            time,
            chapter_at_time(&chapters, time),
            thumbnail,
        );
    });

    let leave_preview = Rc::clone(&preview);
    motion.connect_leave(move |_| {
        leave_preview.hide();
    });

    seek.add_controller(motion);
    preview
}

pub(crate) fn seek_hover_snapshot(
    state: &Rc<RefCell<PlayerState>>,
) -> Option<(Option<PathBuf>, f64, Vec<Chapter>)> {
    let state = state.borrow();
    let thumbnail_source =
        seek_hover_source(state.current_file.clone(), state.current_url.as_deref())?;

    state
        .mpv
        .as_ref()
        .map(|mpv| mpv.observed_playback_state())
        .and_then(|playback| playback.duration)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (thumbnail_source, duration, state.chapters_snapshot.clone()))
}

/// Resolve the hover-preview source for the loaded media: `None` when nothing is
/// loaded (no preview at all), `Some(Some(file))` for a local file that can be
/// sampled for a thumbnail, and `Some(None)` for a stream — which still previews the
/// timecode and chapter but has no on-disk file to thumbnail.
pub(crate) fn seek_hover_source(
    current_file: Option<PathBuf>,
    current_url: Option<&str>,
) -> Option<Option<PathBuf>> {
    if current_file.is_none() && current_url.is_none() {
        return None;
    }
    Some(current_file)
}

pub(crate) fn chapter_at_time(chapters: &[Chapter], time: f64) -> Option<&Chapter> {
    let mut current = None;
    for chapter in chapters {
        if chapter.time.is_finite() && chapter.time <= time {
            current = Some(chapter);
        } else {
            break;
        }
    }

    current
}

pub(crate) fn hover_thumbnail_for_time(
    state: &Rc<RefCell<PlayerState>>,
    media_path: &Path,
    time: f64,
    duration: f64,
    sender: &mpsc::Sender<String>,
) -> Option<PathBuf> {
    let thumbnail_time = thumbnails::hover_thumbnail_time(time, duration);
    if let Some(path) = thumbnails::existing_hover_thumbnail_path(media_path, thumbnail_time) {
        return Some(path);
    }

    let request_key = thumbnails::hover_request_key(media_path, thumbnail_time);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.hover_thumbnail_request_key.as_deref() == Some(request_key.as_str()) {
            false
        } else {
            state.hover_thumbnail_request_key = Some(request_key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_hover_thumbnail(
            media_path.to_path_buf(),
            thumbnail_time,
            request_key,
            sender.clone(),
        );
    }

    None
}

/// Visual smoke hook: pop the seek hover tooltip over the timeline with a
/// representative timecode and chapter and no thumbnail — the deliberate
/// timecode-only fallback the tooltip shows for a stream, a not-yet-generated frame,
/// or an unavailable source. Presentational only; production code never calls this.
/// The pop is deferred so the seek scale has a real allocation to anchor against.
pub(crate) fn open_seek_preview(controls: &Controls) {
    let seek = controls.seek.clone();
    let preview = Rc::clone(&controls.seek_hover_preview);
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let width = f64::from(seek.width().max(1));
        let chapter = Chapter {
            index: 2,
            time: 933.0,
            title: Some("The Long Walk Home".to_owned()),
        };
        preview.show(&seek, width / 2.0, chapter.time, Some(&chapter), None);
    });
}
