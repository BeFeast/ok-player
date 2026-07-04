use super::*;

pub(crate) fn populate_subtitle_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let content = track_popover_content("Subtitles");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);
    let secondary_subtitle_id = read_secondary_subtitle_id(&state);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_subtitle(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let button = track_button(&track_label(track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_subtitle(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    content.append(&track_group_title("Secondary"));

    let secondary_off_button = track_button("Off", secondary_subtitle_id.is_none());
    let secondary_off_state = Rc::clone(&state);
    let secondary_off_popover = popover.clone();
    secondary_off_button.connect_clicked(move |_| {
        if with_mpv(&secondary_off_state, |mpv| {
            mpv.select_secondary_subtitle(None)
        }) {
            save_current_preferences(&secondary_off_state);
        }
        secondary_off_popover.popdown();
    });
    content.append(&secondary_off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let selected = secondary_subtitle_id == Some(track.id);
            let button = track_button(&track_label(track), selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| {
                    mpv.select_secondary_subtitle(Some(track_id))
                }) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    let add_button = track_button("Add subtitle file...", false);
    let add_state = Rc::clone(&state);
    let add_parent = parent.clone();
    let add_popover = popover.clone();
    add_button.connect_clicked(move |_| {
        add_popover.popdown();
        open_subtitle_dialog(&add_parent, Rc::clone(&add_state));
    });
    content.append(&add_button);

    content.append(&divider());
    content.append(&subtitle_adjustment_rows(popover, parent, &state));

    set_track_popover_child(popover, content);
}

pub(crate) fn populate_audio_popover(
    popover: &gtk::Popover,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = track_popover_content("Audio");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_audio(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No audio tracks"));
    } else {
        for track in tracks {
            let button = track_button(&track_label(&track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_audio(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    content.append(&track_group_title("Output Device"));
    let devices = read_audio_devices(&state);
    if devices.is_empty() {
        content.append(&empty_track_label("No output devices"));
    } else {
        for device in devices {
            let button = track_button(&device.label, device.selected);
            let device_state = Rc::clone(&state);
            let device_popover = popover.clone();
            let device_toast = Rc::clone(&status_toast);
            let device_name = device.name.clone();
            let device_label = device.label.clone();
            button.connect_clicked(move |_| {
                if with_mpv(&device_state, |mpv| mpv.set_audio_device(&device_name)) {
                    save_audio_device_setting(
                        &device_state,
                        &device_name,
                        Some(device_toast.as_ref()),
                    );
                    device_toast.show(&format!("Audio output: {device_label}"));
                }
                device_popover.popdown();
            });
            content.append(&button);
        }
    }

    set_track_popover_child(popover, content);
}

pub(crate) fn populate_speed_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content("Speed");
    let current_speed = read_playback_speed(&state);

    for speed in SPEED_PRESETS {
        let button = track_button(&format_speed(speed), speed_matches(current_speed, speed));
        let speed_state = Rc::clone(&state);
        let speed_popover = popover.clone();
        button.connect_clicked(move |_| {
            set_playback_speed_from_ui(&speed_state, speed);
            speed_popover.popdown();
        });
        content.append(&button);
    }

    set_track_popover_child(popover, content);
}

pub(crate) fn populate_command_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = command_popover_content(popover, parent, state, status_toast);
    set_track_popover_child(popover, content);
}

pub(crate) fn command_popover_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = track_popover_content("More");
    let (
        has_media,
        repeat_mode,
        shuffle_enabled,
        auto_advance_enabled,
        private_session,
        playlist_count,
        has_local_media,
        video_transform,
        ab_loop_active,
    ) = {
        let state = state.borrow();
        (
            has_loaded_media_state(&state),
            state.playlist.repeat(),
            state.playlist.shuffle(),
            state.playlist.auto_advance(),
            state.private_session,
            state.playlist.len(),
            state.current_file.is_some(),
            state.video_transform.clone(),
            state.ab_loop.is_active(),
        )
    };

    let open_url_button = track_button("Open URL...", false);
    let open_url_parent = parent.clone();
    let open_url_state = Rc::clone(&state);
    let open_url_toast = Rc::clone(&status_toast);
    let open_url_popover = popover.clone();
    open_url_button.connect_clicked(move |_| {
        open_url_popover.popdown();
        open_url_dialog(
            &open_url_parent,
            Rc::clone(&open_url_state),
            Rc::clone(&open_url_toast),
        );
    });
    content.append(&open_url_button);

    let open_folder_button = track_button("Open Folder...", false);
    let open_folder_parent = parent.clone();
    let open_folder_state = Rc::clone(&state);
    let open_folder_toast = Rc::clone(&status_toast);
    let open_folder_popover = popover.clone();
    open_folder_button.connect_clicked(move |_| {
        open_folder_popover.popdown();
        open_folder_dialog(
            &open_folder_parent,
            Rc::clone(&open_folder_state),
            Rc::clone(&open_folder_toast),
        );
    });
    content.append(&open_folder_button);

    let open_playlist_button = track_button("Open Playlist...", false);
    let open_playlist_parent = parent.clone();
    let open_playlist_state = Rc::clone(&state);
    let open_playlist_toast = Rc::clone(&status_toast);
    let open_playlist_popover = popover.clone();
    open_playlist_button.connect_clicked(move |_| {
        open_playlist_popover.popdown();
        open_playlist_dialog(
            &open_playlist_parent,
            Rc::clone(&open_playlist_state),
            Rc::clone(&open_playlist_toast),
        );
    });
    content.append(&open_playlist_button);

    let add_queue_button = track_button("Add to Queue...", false);
    add_queue_button.set_sensitive(has_local_media);
    add_queue_button.set_tooltip_text(Some("Append local media files to Up Next"));
    let add_queue_parent = parent.clone();
    let add_queue_state = Rc::clone(&state);
    let add_queue_toast = Rc::clone(&status_toast);
    let add_queue_popover = popover.clone();
    add_queue_button.connect_clicked(move |_| {
        add_queue_popover.popdown();
        open_queue_media_dialog(
            &add_queue_parent,
            Rc::clone(&add_queue_state),
            Rc::clone(&add_queue_toast),
            QueueInsertMode::Append,
        );
    });
    content.append(&add_queue_button);

    let play_next_button = track_button("Play Next...", false);
    play_next_button.set_sensitive(has_local_media);
    play_next_button.set_tooltip_text(Some("Insert local media files after the current item"));
    let play_next_parent = parent.clone();
    let play_next_state = Rc::clone(&state);
    let play_next_toast = Rc::clone(&status_toast);
    let play_next_popover = popover.clone();
    play_next_button.connect_clicked(move |_| {
        play_next_popover.popdown();
        open_queue_media_dialog(
            &play_next_parent,
            Rc::clone(&play_next_state),
            Rc::clone(&play_next_toast),
            QueueInsertMode::PlayNext,
        );
    });
    content.append(&play_next_button);

    let save_playlist_button = track_button("Save Playlist...", false);
    save_playlist_button.set_sensitive(playlist_count > 0);
    save_playlist_button.set_tooltip_text(Some("Save current Up Next list as M3U"));
    let save_playlist_parent = parent.clone();
    let save_playlist_state = Rc::clone(&state);
    let save_playlist_toast = Rc::clone(&status_toast);
    let save_playlist_popover = popover.clone();
    save_playlist_button.connect_clicked(move |_| {
        save_playlist_popover.popdown();
        save_playlist_dialog(
            &save_playlist_parent,
            Rc::clone(&save_playlist_state),
            Rc::clone(&save_playlist_toast),
        );
    });
    content.append(&save_playlist_button);

    let settings_button = track_button("Settings...", false);
    let settings_parent = parent.clone();
    let settings_state = Rc::clone(&state);
    let settings_toast = Rc::clone(&status_toast);
    let settings_popover = popover.clone();
    settings_button.connect_clicked(move |_| {
        settings_popover.popdown();
        open_settings_window(
            &settings_parent,
            Rc::clone(&settings_state),
            Rc::clone(&settings_toast),
        );
    });
    content.append(&settings_button);

    let info_button = track_button("Media Info...", false);
    info_button.set_sensitive(has_media);
    info_button.set_tooltip_text(Some("Media Information (I)"));
    let info_parent = parent.clone();
    let info_state = Rc::clone(&state);
    let info_toast = Rc::clone(&status_toast);
    let info_popover = popover.clone();
    info_button.connect_clicked(move |_| {
        info_popover.popdown();
        open_media_info_window(&info_parent, &info_state, Rc::clone(&info_toast));
    });
    content.append(&info_button);

    let location_button = track_button("Open File Location", false);
    location_button.set_sensitive(has_local_media);
    location_button.set_tooltip_text(Some("Open the current file in the file manager"));
    let location_state = Rc::clone(&state);
    let location_toast = Rc::clone(&status_toast);
    let location_popover = popover.clone();
    location_button.connect_clicked(move |_| {
        location_popover.popdown();
        open_current_file_location(&location_state, &location_toast);
    });
    content.append(&location_button);

    let go_to_time_button = track_button("Go to Time...", false);
    go_to_time_button.set_sensitive(has_media);
    go_to_time_button.set_tooltip_text(Some("Go to timecode (J)"));
    let go_to_time_parent = parent.clone();
    let go_to_time_state = Rc::clone(&state);
    let go_to_time_toast = Rc::clone(&status_toast);
    let go_to_time_popover = popover.clone();
    go_to_time_button.connect_clicked(move |_| {
        go_to_time_popover.popdown();
        open_go_to_time_dialog(
            &go_to_time_parent,
            Rc::clone(&go_to_time_state),
            Rc::clone(&go_to_time_toast),
        );
    });
    content.append(&go_to_time_button);

    let copy_time_button = track_button("Copy Current Time", false);
    copy_time_button.set_sensitive(has_media);
    copy_time_button.set_tooltip_text(Some("Copy the current timecode"));
    let copy_time_state = Rc::clone(&state);
    let copy_time_toast = Rc::clone(&status_toast);
    let copy_time_popover = popover.clone();
    copy_time_button.connect_clicked(move |_| {
        copy_time_popover.popdown();
        copy_current_time(&copy_time_state, &copy_time_toast);
    });
    content.append(&copy_time_button);

    let add_bookmark_button = track_button("Add Bookmark", false);
    add_bookmark_button.set_sensitive(has_local_media);
    add_bookmark_button.set_tooltip_text(Some("Save a bookmark at the current position"));
    let add_bookmark_state = Rc::clone(&state);
    let add_bookmark_toast = Rc::clone(&status_toast);
    let add_bookmark_popover = popover.clone();
    add_bookmark_button.connect_clicked(move |_| {
        add_bookmark_popover.popdown();
        add_bookmark_at_position(&add_bookmark_state, &add_bookmark_toast);
    });
    content.append(&add_bookmark_button);

    let ab_loop_button = track_button("A-B loop", ab_loop_active);
    ab_loop_button.set_sensitive(has_media);
    ab_loop_button.set_tooltip_text(Some("Set A, set B, clear (L)"));
    let ab_loop_state = Rc::clone(&state);
    let ab_loop_toast = Rc::clone(&status_toast);
    let ab_loop_popover = popover.clone();
    ab_loop_button.connect_clicked(move |_| {
        ab_loop_popover.popdown();
        toggle_ab_loop(&ab_loop_state, &ab_loop_toast);
    });
    content.append(&ab_loop_button);

    content.append(&divider());
    content.append(&track_group_title("Video"));
    content.append(&track_subgroup_title("Aspect ratio"));
    for (label, aspect) in VIDEO_ASPECT_PRESETS {
        let button = track_button(label, video_transform.aspect_override == aspect);
        button.set_sensitive(has_media);
        let aspect_state = Rc::clone(&state);
        let aspect_toast = Rc::clone(&status_toast);
        let aspect_popover = popover.clone();
        button.connect_clicked(move |_| {
            aspect_popover.popdown();
            set_video_aspect(&aspect_state, aspect, &aspect_toast);
        });
        content.append(&button);
    }

    let rotate_button = track_button("Rotate 90°", false);
    rotate_button.set_sensitive(has_media);
    let rotate_state = Rc::clone(&state);
    let rotate_toast = Rc::clone(&status_toast);
    let rotate_popover = popover.clone();
    rotate_button.connect_clicked(move |_| {
        rotate_popover.popdown();
        rotate_video_clockwise(&rotate_state, &rotate_toast);
    });
    content.append(&rotate_button);

    let fill_button = track_button("Fill screen (crop bars)", video_transform.fill_screen);
    fill_button.set_sensitive(has_media);
    let fill_state = Rc::clone(&state);
    let fill_toast = Rc::clone(&status_toast);
    let fill_popover = popover.clone();
    fill_button.connect_clicked(move |_| {
        fill_popover.popdown();
        toggle_video_fill_screen(&fill_state, &fill_toast);
    });
    content.append(&fill_button);

    let reset_video_button = track_button("Reset video", false);
    reset_video_button.set_sensitive(has_media);
    let reset_video_state = Rc::clone(&state);
    let reset_video_toast = Rc::clone(&status_toast);
    let reset_video_popover = popover.clone();
    reset_video_button.connect_clicked(move |_| {
        reset_video_popover.popdown();
        reset_video_transform(&reset_video_state, &reset_video_toast);
    });
    content.append(&reset_video_button);

    content.append(&divider());
    content.append(&track_group_title("Screenshot"));

    let save_frame_button = track_button("Save frame", false);
    save_frame_button.set_sensitive(has_media);
    let save_frame_state = Rc::clone(&state);
    let save_frame_toast = Rc::clone(&status_toast);
    let save_frame_popover = popover.clone();
    save_frame_button.connect_clicked(move |_| {
        save_frame_popover.popdown();
        save_screenshot(&save_frame_state, &save_frame_toast, false);
    });
    content.append(&save_frame_button);

    let save_subs_button = track_button("Save frame with subtitles", false);
    save_subs_button.set_sensitive(has_media);
    let save_subs_state = Rc::clone(&state);
    let save_subs_toast = Rc::clone(&status_toast);
    let save_subs_popover = popover.clone();
    save_subs_button.connect_clicked(move |_| {
        save_subs_popover.popdown();
        save_screenshot(&save_subs_state, &save_subs_toast, true);
    });
    content.append(&save_subs_button);

    let copy_frame_button = track_button("Copy frame to clipboard", false);
    copy_frame_button.set_sensitive(has_media);
    let copy_frame_state = Rc::clone(&state);
    let copy_frame_toast = Rc::clone(&status_toast);
    let copy_frame_popover = popover.clone();
    copy_frame_button.connect_clicked(move |_| {
        copy_frame_popover.popdown();
        copy_frame_to_clipboard(&copy_frame_state, &copy_frame_toast);
    });
    content.append(&copy_frame_button);

    let close_button = track_button("Close Media", false);
    close_button.set_sensitive(has_media);
    let close_state = Rc::clone(&state);
    let close_toast = Rc::clone(&status_toast);
    let close_popover = popover.clone();
    close_button.connect_clicked(move |_| {
        close_popover.popdown();
        close_current_media(&close_state, &close_toast);
    });
    content.append(&close_button);

    let fullscreen_label = if parent.is_fullscreen() {
        "Exit Fullscreen"
    } else {
        "Enter Fullscreen"
    };
    let fullscreen_button = track_button(fullscreen_label, parent.is_fullscreen());
    let fullscreen_parent = parent.clone();
    let fullscreen_popover = popover.clone();
    fullscreen_button.connect_clicked(move |_| {
        fullscreen_popover.popdown();
        toggle_fullscreen(&fullscreen_parent);
    });
    content.append(&fullscreen_button);

    content.append(&divider());

    let private_button = track_button(
        if private_session {
            "Private Session On"
        } else {
            "Private Session Off"
        },
        private_session,
    );
    let private_state = Rc::clone(&state);
    let private_toast = Rc::clone(&status_toast);
    let private_popover = popover.clone();
    private_button.connect_clicked(move |_| {
        toggle_private_session(&private_state, &private_toast);
        private_popover.popdown();
    });
    content.append(&private_button);

    let clear_history_button = track_button("Clear History...", false);
    let clear_history_parent = parent.clone();
    let clear_history_state = Rc::clone(&state);
    let clear_history_toast = Rc::clone(&status_toast);
    let clear_history_popover = popover.clone();
    clear_history_button.connect_clicked(move |_| {
        clear_history_popover.popdown();
        open_clear_history_dialog(
            &clear_history_parent,
            Rc::clone(&clear_history_state),
            Rc::clone(&clear_history_toast),
        );
    });
    content.append(&clear_history_button);

    content.append(&divider());

    let repeat_button = track_button(
        repeat_mode_label(repeat_mode),
        repeat_mode != RepeatMode::Off,
    );
    let repeat_state = Rc::clone(&state);
    let repeat_toast = Rc::clone(&status_toast);
    let repeat_popover = popover.clone();
    repeat_button.connect_clicked(move |_| {
        cycle_repeat_mode(&repeat_state, &repeat_toast);
        repeat_popover.popdown();
    });
    content.append(&repeat_button);

    let shuffle_button = track_button(
        if shuffle_enabled {
            "Shuffle On"
        } else {
            "Shuffle Off"
        },
        shuffle_enabled,
    );
    let shuffle_state = Rc::clone(&state);
    let shuffle_toast = Rc::clone(&status_toast);
    let shuffle_popover = popover.clone();
    shuffle_button.connect_clicked(move |_| {
        toggle_shuffle(&shuffle_state, &shuffle_toast);
        shuffle_popover.popdown();
    });
    content.append(&shuffle_button);

    let auto_advance_button = track_button(
        if auto_advance_enabled {
            "Auto-advance On"
        } else {
            "Auto-advance Off"
        },
        auto_advance_enabled,
    );
    let auto_advance_state = Rc::clone(&state);
    let auto_advance_toast = Rc::clone(&status_toast);
    let auto_advance_popover = popover.clone();
    auto_advance_button.connect_clicked(move |_| {
        toggle_auto_advance(&auto_advance_state, &auto_advance_toast);
        auto_advance_popover.popdown();
    });
    content.append(&auto_advance_button);

    content
}

pub(crate) fn track_popover_content(title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.add_css_class("okp-track-popover-content");
    content.set_width_request(320);

    content.append(&track_section_title(title));
    content
}

pub(crate) fn set_track_popover_child(popover: &gtk::Popover, content: gtk::Box) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("okp-track-popover-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_width(320);
    scroll.set_max_content_height(520);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&content));
    popover.set_child(Some(&scroll));
}

pub(crate) fn track_section_title(title: &str) -> gtk::Label {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-track-popover-title");
    title.set_xalign(0.0);
    title
}

/// An "eyebrow" header for a group inside a popover (e.g. Secondary, Output
/// Device, Screenshot) — dimmer and smaller than the popover title so the
/// primary heading stays dominant and the sections read as a clear hierarchy.
pub(crate) fn track_group_title(title: &str) -> gtk::Label {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-track-group-title");
    title.set_xalign(0.0);
    title
}

/// A quieter sub-header nested under a group header (e.g. Aspect ratio under
/// Video) so two stacked headers do not read as siblings.
pub(crate) fn track_subgroup_title(title: &str) -> gtk::Label {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-track-subgroup-title");
    title.set_xalign(0.0);
    title
}

pub(crate) fn subtitle_adjustment_rows(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);

    let (delay_seconds, scale) = read_subtitle_adjustments(state);
    content.append(&subtitle_delay_adjustment_row(
        delay_seconds,
        popover,
        parent,
        state,
    ));
    content.append(&subtitle_adjustment_row(
        "Size",
        &format_scale(scale),
        [
            ("-", SubtitleAdjustment::Scale(-0.1)),
            ("100%", SubtitleAdjustment::SetScale(1.0)),
            ("+", SubtitleAdjustment::Scale(0.1)),
        ],
        popover,
        parent,
        state,
    ));

    content
}

pub(crate) fn subtitle_delay_adjustment_row(
    delay_seconds: f64,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("okp-sub-adjust-row");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some("Delay"));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    top.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&subtitle_delay::format_entry(delay_seconds));
    entry.set_width_chars(8);
    entry.set_placeholder_text(Some("0"));
    top.append(&entry);

    let unit = gtk::Label::new(Some("ms"));
    unit.add_css_class("okp-sub-adjust-unit");
    top.append(&unit);

    let apply_button = gtk::Button::with_label("Apply");
    apply_button.add_css_class("okp-sub-adjust-button");
    top.append(&apply_button);

    let reset_button = gtk::Button::with_label("Reset");
    reset_button.add_css_class("okp-sub-adjust-button");
    top.append(&reset_button);

    row.append(&top);

    let quick = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    quick.set_halign(gtk::Align::End);
    for (text, adjustment) in [
        ("-50", SubtitleAdjustment::Delay(-0.05)),
        ("+50", SubtitleAdjustment::Delay(0.05)),
    ] {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        quick.append(&button);
    }
    row.append(&quick);

    let apply_state = Rc::clone(state);
    let apply_popover = popover.clone();
    let apply_parent = parent.clone();
    let apply_entry = entry.clone();
    apply_button.connect_clicked(move |_| {
        apply_subtitle_delay_entry(
            &apply_entry,
            &apply_popover,
            &apply_parent,
            Rc::clone(&apply_state),
        );
    });

    let activate_state = Rc::clone(state);
    let activate_popover = popover.clone();
    let activate_parent = parent.clone();
    entry.connect_activate(move |entry| {
        apply_subtitle_delay_entry(
            entry,
            &activate_popover,
            &activate_parent,
            Rc::clone(&activate_state),
        );
    });

    let reset_state = Rc::clone(state);
    let reset_popover = popover.clone();
    let reset_parent = parent.clone();
    reset_button.connect_clicked(move |_| {
        apply_subtitle_adjustment(&reset_state, SubtitleAdjustment::SetDelay(0.0));
        populate_subtitle_popover(&reset_popover, &reset_parent, Rc::clone(&reset_state));
    });

    entry.connect_changed(|entry| {
        entry.remove_css_class("is-error");
    });

    row
}

pub(crate) fn subtitle_adjustment_row(
    title: &str,
    value: &str,
    actions: [(&str, SubtitleAdjustment); 3],
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("okp-sub-adjust-row");

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    row.append(&label);

    let value_label = gtk::Label::new(Some(value));
    value_label.add_css_class("okp-sub-adjust-value");
    value_label.set_xalign(1.0);
    value_label.set_width_chars(7);
    row.append(&value_label);

    for (text, adjustment) in actions {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        row.append(&button);
    }

    row
}

pub(crate) fn read_subtitle_adjustments(state: &Rc<RefCell<PlayerState>>) -> (f64, f64) {
    let values = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| (mpv.observed_subtitle_delay(), mpv.observed_subtitle_scale()))
    };

    values.unwrap_or((0.0, 1.0))
}

pub(crate) fn apply_subtitle_adjustment(
    state: &Rc<RefCell<PlayerState>>,
    adjustment: SubtitleAdjustment,
) {
    if with_mpv(state, |mpv| match adjustment {
        SubtitleAdjustment::Delay(delta) => mpv.adjust_subtitle_delay(delta),
        SubtitleAdjustment::SetDelay(value) => mpv.set_subtitle_delay(value),
        SubtitleAdjustment::Scale(delta) => mpv.adjust_subtitle_scale(delta),
        SubtitleAdjustment::SetScale(value) => mpv.set_subtitle_scale(value),
    }) {
        save_current_preferences(state);
    }
}

pub(crate) fn apply_subtitle_delay_entry(
    entry: &gtk::Entry,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let Some(delay_seconds) = subtitle_delay::parse_entry_seconds(entry.text().as_str()) else {
        entry.add_css_class("is-error");
        entry.grab_focus();
        return;
    };

    apply_subtitle_adjustment(&state, SubtitleAdjustment::SetDelay(delay_seconds));
    populate_subtitle_popover(popover, parent, state);
}

pub(crate) fn format_scale(scale: f64) -> String {
    format!("{:.0}%", scale * 100.0)
}

pub(crate) fn read_tracks(state: &Rc<RefCell<PlayerState>>) -> Vec<Track> {
    let state = state.borrow();
    state
        .mpv
        .as_ref()
        .map(Mpv::observed_tracks)
        .unwrap_or_default()
}

pub(crate) fn read_audio_devices(state: &Rc<RefCell<PlayerState>>) -> Vec<AudioDevice> {
    let state = state.borrow();
    state
        .mpv
        .as_ref()
        .map(Mpv::observed_audio_devices)
        .unwrap_or_default()
}

pub(crate) fn schedule_audio_device_restore(state: &Rc<RefCell<PlayerState>>) {
    let device = state.borrow().settings.audio_device().trim().to_owned();
    state.borrow_mut().pending_audio_device_restore =
        should_restore_audio_device(&device).then(|| PendingAudioDeviceRestore::new(device));
}

pub(crate) fn try_pending_audio_device_restore(state: &Rc<RefCell<PlayerState>>) {
    let Some(pending) = state.borrow().pending_audio_device_restore.clone() else {
        return;
    };

    let restore_result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.restore_audio_device(&pending.name)
    };

    match restore_result {
        Ok(true) => state.borrow_mut().pending_audio_device_restore = None,
        Ok(false) => record_audio_device_restore_miss(state, pending, None),
        Err(error) => record_audio_device_restore_miss(state, pending, Some(error.to_string())),
    }
}

pub(crate) fn record_audio_device_restore_miss(
    state: &Rc<RefCell<PlayerState>>,
    pending: PendingAudioDeviceRestore,
    error: Option<String>,
) {
    let next = next_audio_device_restore_retry(pending.clone(), AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS);
    if next.is_none() {
        if let Some(error) = error {
            eprintln!(
                "Failed to restore saved audio output '{}': {error}",
                pending.name
            );
        } else {
            eprintln!(
                "Saved audio output '{}' is not available after {AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS} attempts",
                pending.name
            );
        }
    }
    state.borrow_mut().pending_audio_device_restore = next;
}

pub(crate) fn should_restore_audio_device(device: &str) -> bool {
    let device = device.trim();
    !device.is_empty() && device != AUDIO_DEVICE_AUTO
}

pub(crate) fn next_audio_device_restore_retry(
    mut pending: PendingAudioDeviceRestore,
    max_attempts: u8,
) -> Option<PendingAudioDeviceRestore> {
    pending.attempts = pending.attempts.saturating_add(1);
    (pending.attempts < max_attempts).then_some(pending)
}

pub(crate) fn read_secondary_subtitle_id(state: &Rc<RefCell<PlayerState>>) -> Option<i64> {
    let state = state.borrow();
    state
        .mpv
        .as_ref()
        .and_then(Mpv::observed_secondary_subtitle_id)
}

pub(crate) fn track_button(text: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-track-row");
    if selected {
        button.add_css_class("is-selected");
    }

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    // A reserved leading column carries the accent check for the selected row.
    // Toggling opacity (rather than visibility) keeps every label aligned on the
    // same left edge whether or not its row is selected, so the "Off"/track list
    // reads as one column instead of jumping when the selection moves.
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("okp-track-check");
    check.set_pixel_size(14);
    check.set_valign(gtk::Align::Center);
    check.set_opacity(if selected { 1.0 } else { 0.0 });
    row.append(&check);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-track-row-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);
    row.append(&label);

    button.set_child(Some(&row));
    button
}

pub(crate) fn empty_track_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-track-empty");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

pub(crate) fn divider() -> gtk::Separator {
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-track-divider");
    divider
}

/// The one-line descriptor for a track row. Selection is shown by the row's
/// leading check (see [`track_button`]), so the text carries only the track's
/// name and its format tags — no "On" prefix that would shift long titles.
pub(crate) fn track_label(track: &Track) -> String {
    let mut parts = Vec::new();
    parts.push(track_base_label(track));

    if track.kind == TrackKind::Audio {
        if let Some(channels) = track.audio_channels.as_deref() {
            parts.push(channels.to_owned());
        }
        if let Some(codec) = track.codec.as_deref() {
            parts.push(codec.to_ascii_uppercase());
        }
    } else if track.external {
        parts.push("EXT".to_owned());
    } else if track.default {
        parts.push("Default".to_owned());
    }

    parts.join(" · ")
}

pub(crate) fn track_base_label(track: &Track) -> String {
    track
        .title
        .as_deref()
        .or(track.lang.as_deref())
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Track {}", track.id))
}

pub(crate) fn drain_mpv_events(state: &Rc<RefCell<PlayerState>>) {
    let events = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(Mpv::take_lifecycle_events)
            .unwrap_or_default()
    };

    for event in events {
        match event {
            MpvEvent::FileLoaded => {
                try_pending_audio_device_restore(state);
                try_pending_playback_preferences(state);
            }
            MpvEvent::EndFile { reason } if reason.is_eof() => {
                if state.borrow().playlist.repeat() != RepeatMode::One {
                    save_current_progress(state, true);
                }
                advance_playlist_on_eof(state);
            }
            _ => {}
        }
    }
}
