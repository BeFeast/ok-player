use super::*;

pub(crate) const SPEED_POPOVER_WIDTH: i32 = 120;
pub(crate) const SUBTITLE_POPOVER_WIDTH: i32 = 262;
pub(crate) const AUDIO_POPOVER_WIDTH: i32 = 248;
pub(crate) const MORE_POPOVER_WIDTH: i32 = 210;
const ADVANCED_COMMAND_POPOVER_WIDTH: i32 = 320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlayerPopoverKind {
    Speed,
    Subtitles,
    Audio,
    More,
    AdvancedCommands,
}

impl PlayerPopoverKind {
    pub(crate) const fn width(self) -> i32 {
        match self {
            Self::Speed => SPEED_POPOVER_WIDTH,
            Self::Subtitles => SUBTITLE_POPOVER_WIDTH,
            Self::Audio => AUDIO_POPOVER_WIDTH,
            Self::More => MORE_POPOVER_WIDTH,
            Self::AdvancedCommands => ADVANCED_COMMAND_POPOVER_WIDTH,
        }
    }

    pub(crate) const fn css_class(self) -> &'static str {
        match self {
            Self::Speed => "okp-speed-popover",
            Self::Subtitles => "okp-subtitle-popover",
            Self::Audio => "okp-audio-popover",
            Self::More => "okp-more-popover",
            Self::AdvancedCommands => "okp-advanced-command-popover",
        }
    }
}

pub(crate) fn populate_subtitle_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = track_popover_content(PlayerPopoverKind::Subtitles, Some("Subtitles"));
    let preview_tracks = preview_tracks(TrackKind::Subtitle);
    let previewing_tracks = preview_tracks.is_some();
    let tracks = preview_tracks
        .unwrap_or_else(|| read_tracks(&state))
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let secondary_subtitle_id = if previewing_tracks {
        None
    } else {
        read_secondary_subtitle_id(&state)
    };
    // mpv reports `selected` for the secondary caption too, so the primary
    // "Off" state and the primary checkmark must exclude the secondary track —
    // otherwise a secondary-only file reads as though a primary were active and
    // the secondary shows a stray check in the primary list. The rule is shared
    // with the Windows shell and lives in okp-core (freeze-boundary).
    let has_primary = okp_core::subtitle_tracks::has_primary_subtitle(
        tracks.iter().map(|track| (track.id, track.selected)),
        secondary_subtitle_id,
    );

    let off_button = track_button("Off", !has_primary);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_subtitle(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    for track in &tracks {
        let selected = okp_core::subtitle_tracks::is_primary_subtitle(
            track.id,
            track.selected,
            secondary_subtitle_id,
        );
        let button = track_button(&track_label(track), selected);
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

    content.append(&divider());
    let current_file = state.borrow().current_file.clone();
    let search_source =
        selected_subtitle_search_source(&tracks, secondary_subtitle_id, current_file.as_deref());
    let search_button = track_button("Search subtitles...", false);
    search_button.set_sensitive(matches!(search_source, SubtitleSearchSource::Available(_)));
    search_button.set_tooltip_text(Some(search_source.message()));
    let search_parent = parent.clone();
    let search_state = Rc::clone(&state);
    let search_toast = Rc::clone(&status_toast);
    let search_popover = popover.clone();
    search_button.connect_clicked(move |_| {
        search_popover.popdown();
        open_subtitle_search_dialog(
            &search_parent,
            Rc::clone(&search_state),
            Rc::clone(&search_toast),
        );
    });
    content.append(&search_button);
    if !matches!(search_source, SubtitleSearchSource::Available(_)) {
        content.append(&empty_track_label(search_source.message()));
    }

    content.append(&scribe_subtitle_button());
    content.append(&compact_subtitle_delay_row(
        read_subtitle_adjustments(&state).0,
        &state,
    ));
    content.append(&compact_subtitle_size_row(
        read_subtitle_adjustments(&state).1,
        &state,
    ));
    content.append(&compact_subtitle_style_row(&state));
    let footer = gtk::Label::new(Some("More in Settings → Subtitles"));
    footer.add_css_class("okp-quick-preference-footer");
    footer.set_xalign(0.0);
    content.append(&footer);

    set_track_popover_child(popover, PlayerPopoverKind::Subtitles, content);
}

pub(crate) fn populate_audio_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content(PlayerPopoverKind::Audio, Some("Audio"));
    let tracks = preview_tracks(TrackKind::Audio)
        .unwrap_or_else(|| read_tracks(&state))
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();

    if tracks.is_empty() {
        content.append(&empty_track_label("No audio tracks"));
    } else {
        for track in tracks {
            let (name, detail) = okp_core::track_label::audio_track_parts(
                track.id,
                track.title.as_deref(),
                track.lang.as_deref(),
                track.audio_channels.as_deref(),
                track.codec.as_deref(),
            );
            let button = audio_track_button(&name, &detail, track.selected);
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

    set_track_popover_child(popover, PlayerPopoverKind::Audio, content);
}

pub(crate) fn populate_speed_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content(PlayerPopoverKind::Speed, Some("Speed"));
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

    set_track_popover_child(popover, PlayerPopoverKind::Speed, content);
}

pub(crate) fn populate_command_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = more_popover_content(popover, parent, state, status_toast);
    set_track_popover_child(popover, PlayerPopoverKind::More, content);
}

pub(crate) fn more_popover_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = track_popover_content(PlayerPopoverKind::More, None);
    let (mut has_media, ab_loop_active) = {
        let state = state.borrow();
        (has_loaded_media_state(&state), state.ab_loop.is_active())
    };
    if env::var_os("OKP_PLAYER_POPOVER_PREVIEW_STATE")
        .is_some_and(|state| state.eq_ignore_ascii_case("more-disabled"))
    {
        has_media = false;
    }

    let open_button = command_button("Open file...", false);
    let open_parent = parent.clone();
    let open_state = Rc::clone(&state);
    let open_toast = Rc::clone(&status_toast);
    let open_popover = popover.clone();
    open_button.connect_clicked(move |_| {
        open_popover.popdown();
        open_media_dialog(&open_parent, Rc::clone(&open_state), Rc::clone(&open_toast));
    });
    content.append(&open_button);

    let close_button = command_button("Close file", false);
    close_button.set_sensitive(has_media);
    let close_state = Rc::clone(&state);
    let close_toast = Rc::clone(&status_toast);
    let close_popover = popover.clone();
    close_button.connect_clicked(move |_| {
        close_popover.popdown();
        close_current_media(&close_state, &close_toast);
    });
    content.append(&close_button);

    let ab_loop_button = command_button("A-B loop", ab_loop_active);
    ab_loop_button.set_sensitive(has_media);
    let ab_loop_state = Rc::clone(&state);
    let ab_loop_toast = Rc::clone(&status_toast);
    let ab_loop_popover = popover.clone();
    ab_loop_button.connect_clicked(move |_| {
        ab_loop_popover.popdown();
        toggle_ab_loop(&ab_loop_state, &ab_loop_toast);
    });
    content.append(&ab_loop_button);

    content.append(&divider());

    let save_subs_button = command_button("Screenshot with subtitles", false);
    save_subs_button.set_sensitive(has_media);
    let save_subs_state = Rc::clone(&state);
    let save_subs_toast = Rc::clone(&status_toast);
    let save_subs_popover = popover.clone();
    save_subs_button.connect_clicked(move |_| {
        save_subs_popover.popdown();
        save_screenshot(&save_subs_state, &save_subs_toast, true);
    });
    content.append(&save_subs_button);

    let copy_frame_button = command_button("Copy frame to clipboard", false);
    copy_frame_button.set_sensitive(has_media);
    let copy_frame_state = Rc::clone(&state);
    let copy_frame_toast = Rc::clone(&status_toast);
    let copy_frame_popover = popover.clone();
    copy_frame_button.connect_clicked(move |_| {
        copy_frame_popover.popdown();
        copy_frame_to_clipboard(&copy_frame_state, &copy_frame_toast);
    });
    content.append(&copy_frame_button);

    content.append(&divider());

    let info_button = command_button("Media info...", false);
    info_button.set_sensitive(has_media);
    let info_parent = parent.clone();
    let info_state = Rc::clone(&state);
    let info_toast = Rc::clone(&status_toast);
    let info_popover = popover.clone();
    info_button.connect_clicked(move |_| {
        info_popover.popdown();
        open_media_info_window(&info_parent, &info_state, Rc::clone(&info_toast));
    });
    content.append(&info_button);

    let settings_button = command_button("Settings...", false);
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

    content
}

pub(crate) fn advanced_command_popover_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = track_popover_content(
        PlayerPopoverKind::AdvancedCommands,
        Some("Advanced commands"),
    );
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

pub(crate) fn track_popover_content(kind: PlayerPopoverKind, title: Option<&str>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.add_css_class("okp-track-popover-content");
    content.add_css_class(kind.css_class());
    content.set_width_request(kind.width());

    if let Some(title) = title {
        content.append(&track_section_title(title));
    }
    content
}

pub(crate) fn set_track_popover_child(
    popover: &gtk::Popover,
    kind: PlayerPopoverKind,
    content: gtk::Box,
) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("okp-track-popover-scroll");
    scroll.add_css_class(kind.css_class());
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_width(kind.width());
    scroll.set_max_content_width(kind.width());
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

pub(crate) fn scribe_subtitle_button() -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-track-row");
    button.add_css_class("okp-scribe-row");
    button.set_sensitive(false);
    button.set_tooltip_text(Some("Scribe subtitle generation is coming later"));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&scribe_icon());

    let label = gtk::Label::new(Some("Generate subtitles (Scribe)"));
    label.add_css_class("okp-track-row-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);
    row.append(&label);
    button.set_child(Some(&row));
    button
}

pub(crate) fn compact_subtitle_delay_row(
    delay_seconds: f64,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("okp-quick-delay-row");

    let label = gtk::Label::new(Some("Delay"));
    label.add_css_class("okp-quick-delay-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let value = gtk::Label::new(Some(&subtitle_delay::format_label(delay_seconds)));
    value.add_css_class("okp-quick-delay-value");
    value.set_width_chars(6);
    value.set_xalign(1.0);
    row.append(&value);
    let projected_delay = Rc::new(Cell::new(delay_seconds));

    for (icon, tooltip, adjustment) in [
        (
            QuickControlIcon::Minus,
            "Move subtitles earlier by 50 ms",
            SubtitleAdjustment::Delay(-0.05),
        ),
        (
            QuickControlIcon::Reset,
            "Reset subtitle delay",
            SubtitleAdjustment::SetDelay(0.0),
        ),
        (
            QuickControlIcon::Plus,
            "Move subtitles later by 50 ms",
            SubtitleAdjustment::Delay(0.05),
        ),
    ] {
        let button = gtk::Button::new();
        button.add_css_class("okp-quick-delay-button");
        button.set_tooltip_text(Some(tooltip));
        button.set_child(Some(&quick_control_icon(icon)));
        let button_state = Rc::clone(state);
        let button_value = value.clone();
        let button_delay = Rc::clone(&projected_delay);
        button.connect_clicked(move |_| {
            if let Some(applied_delay) =
                apply_subtitle_adjustment(&button_state, adjustment, button_delay.get())
            {
                button_delay.set(applied_delay);
                button_value.set_text(&subtitle_delay::format_label(applied_delay));
            }
        });
        row.append(&button);
    }

    row
}

pub(crate) fn compact_subtitle_size_row(scale: f64, state: &Rc<RefCell<PlayerState>>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("okp-quick-delay-row");

    let label = gtk::Label::new(Some("Size"));
    label.add_css_class("okp-quick-delay-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let value = gtk::Label::new(Some(&format_scale(scale)));
    value.add_css_class("okp-quick-delay-value");
    value.set_width_chars(5);
    value.set_xalign(1.0);
    row.append(&value);
    let projected_scale = Rc::new(Cell::new(scale));

    for (icon, tooltip, delta) in [
        (QuickControlIcon::Minus, "Make subtitles smaller", -0.1),
        (QuickControlIcon::Reset, "Reset subtitle size", 0.0),
        (QuickControlIcon::Plus, "Make subtitles larger", 0.1),
    ] {
        let button = gtk::Button::new();
        button.add_css_class("okp-quick-delay-button");
        button.set_tooltip_text(Some(tooltip));
        button.set_child(Some(&quick_control_icon(icon)));
        let button_state = Rc::clone(state);
        let button_value = value.clone();
        let button_scale = Rc::clone(&projected_scale);
        button.connect_clicked(move |_| {
            let target = if delta == 0.0 {
                okp_core::subtitle_style::DEFAULT_SCALE
            } else {
                okp_core::subtitle_style::normalized_scale(Some(button_scale.get() + delta))
            };
            if set_current_subtitle_scale(&button_state, target) {
                button_scale.set(target);
                button_value.set_text(&format_scale(target));
            }
        });
        row.append(&button);
    }

    row
}

pub(crate) fn compact_subtitle_style_row(state: &Rc<RefCell<PlayerState>>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("okp-quick-style-row");

    let label = gtk::Label::new(Some("Style"));
    label.add_css_class("okp-quick-delay-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let current = state.borrow().settings.subtitle_style();
    let button = gtk::Button::with_label(&format!("{}  ›", subtitle_style_label(current.key)));
    button.add_css_class("okp-quick-style-button");
    let button_state = Rc::clone(state);
    let button_label = button.clone();
    button.connect_clicked(move |_| {
        let current = button_state.borrow().settings.subtitle_style();
        let next = okp_core::subtitle_style::next(current);
        if set_subtitle_style_setting(&button_state, next.key).is_ok() {
            button_label.set_label(&format!("{}  ›", subtitle_style_label(next.key)));
        }
    });
    row.append(&button);
    row
}

pub(crate) fn subtitle_style_label(key: &str) -> &'static str {
    match key {
        "Bold" => "Bold",
        "Classic" => "Classic",
        "Contrast" => "High contrast",
        _ => "Default",
    }
}

pub(crate) fn set_current_subtitle_scale(state: &Rc<RefCell<PlayerState>>, scale: f64) -> bool {
    if with_mpv(state, |mpv| mpv.set_subtitle_scale(scale)) {
        save_current_preferences_with_subtitle_scale(state, scale);
        true
    } else {
        false
    }
}

#[derive(Clone, Copy)]
pub(crate) enum QuickControlIcon {
    Minus,
    Reset,
    Plus,
}

pub(crate) fn quick_control_icon(kind: QuickControlIcon) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(12);
    icon.set_content_height(12);
    icon.set_draw_func(move |_, cr, width, height| {
        let width = f64::from(width);
        let height = f64::from(height);
        cr.set_source_rgba(0.10, 0.12, 0.14, 0.78);
        cr.set_line_width(1.35);
        cr.set_line_cap(cairo::LineCap::Round);
        match kind {
            QuickControlIcon::Minus => {
                cr.move_to(width * 0.25, height * 0.5);
                cr.line_to(width * 0.75, height * 0.5);
            }
            QuickControlIcon::Plus => {
                cr.move_to(width * 0.25, height * 0.5);
                cr.line_to(width * 0.75, height * 0.5);
                cr.move_to(width * 0.5, height * 0.25);
                cr.line_to(width * 0.5, height * 0.75);
            }
            QuickControlIcon::Reset => {
                cr.arc(width * 0.52, height * 0.52, width * 0.30, -2.45, 2.15);
                cr.move_to(width * 0.18, height * 0.22);
                cr.line_to(width * 0.18, height * 0.48);
                cr.line_to(width * 0.42, height * 0.43);
            }
        }
        let _ = cr.stroke();
    });
    icon
}

pub(crate) fn scribe_icon() -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.add_css_class("okp-scribe-icon");
    icon.set_content_width(16);
    icon.set_content_height(16);
    icon.set_draw_func(|_, cr, width, height| {
        let width = f64::from(width);
        let height = f64::from(height);
        cr.set_source_rgba(0.06, 0.49, 0.46, 0.56);
        cr.set_line_width(1.35);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        cairo_rounded_rect(cr, 1.5, 3.0, width - 4.0, height - 6.0, 2.0);
        let _ = cr.stroke();
        cr.move_to(4.0, height - 3.0);
        cr.line_to(4.0, height - 0.8);
        cr.line_to(7.0, height - 3.0);
        let _ = cr.stroke();
        cr.move_to(width - 2.4, 0.8);
        cr.line_to(width - 2.4, 5.2);
        cr.move_to(width - 4.6, 3.0);
        cr.line_to(width - 0.2, 3.0);
        let _ = cr.stroke();
    });
    icon
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
    projected_delay: f64,
) -> Option<f64> {
    let applied_delay = subtitle_delay_target(projected_delay, adjustment);

    if with_mpv(state, |mpv| match adjustment {
        SubtitleAdjustment::Delay(_) | SubtitleAdjustment::SetDelay(_) => {
            mpv.set_subtitle_delay(applied_delay.unwrap_or_default())
        }
    }) {
        if let Some(delay) = applied_delay {
            save_current_preferences_with_subtitle_delay(state, delay);
        } else {
            save_current_preferences(state);
        }
        applied_delay
    } else {
        None
    }
}

pub(crate) fn subtitle_delay_target(
    current_delay: f64,
    adjustment: SubtitleAdjustment,
) -> Option<f64> {
    match adjustment {
        SubtitleAdjustment::Delay(delta) => Some((current_delay + delta).clamp(
            -subtitle_delay::MAX_ENTRY_SECONDS,
            subtitle_delay::MAX_ENTRY_SECONDS,
        )),
        SubtitleAdjustment::SetDelay(value) => Some(value.clamp(
            -subtitle_delay::MAX_ENTRY_SECONDS,
            subtitle_delay::MAX_ENTRY_SECONDS,
        )),
    }
}

pub(crate) fn format_scale(scale: f64) -> String {
    format!("{:.0}%", scale * 100.0)
}

pub(crate) fn read_audio_delay(state: &Rc<RefCell<PlayerState>>) -> f64 {
    state
        .borrow()
        .mpv
        .as_ref()
        .map(Mpv::observed_audio_delay)
        .unwrap_or(0.0)
}

/// The OSD line echoed on an audio-delay change. Reuses the subtitle-delay
/// readout format (`Audio delay: +250 ms`) so both sync nudges read the same,
/// with an "Audio delay:" prefix that makes the surface unmistakable.
pub(crate) fn audio_delay_toast(seconds: f64) -> String {
    format!("Audio delay: {}", subtitle_delay::format_label(seconds))
}

/// Clamp an audio delay to the same ±ten-minute range the entry and the mpv
/// setter accept, so the echoed value and the stored value never diverge.
pub(crate) fn clamp_audio_delay(seconds: f64) -> f64 {
    seconds.clamp(
        -subtitle_delay::MAX_ENTRY_SECONDS,
        subtitle_delay::MAX_ENTRY_SECONDS,
    )
}

/// Set the audio delay to an absolute value through the shared runtime path: the
/// mpv command, then the persisted per-file preference, then the OSD echo. Only
/// the audio delay is touched, never the subtitle delay. Returns the clamped
/// value that was applied so the caller can reflect it immediately — the pump
/// snapshot refreshes asynchronously, so the row cannot read the new value back
/// right away.
pub(crate) fn apply_audio_delay(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
    target_seconds: f64,
) -> Option<f64> {
    let target = clamp_audio_delay(target_seconds);
    if with_mpv(state, |mpv| mpv.set_audio_delay(target)) {
        // Persist the value we just applied. `observed_audio_delay()` reads the
        // async pump snapshot, which may still hold the previous delay, so a
        // reset to `0` could otherwise re-save the old delay.
        save_current_preferences_with_audio_delay(state, target);
        status_toast.show(&audio_delay_toast(target));
        Some(target)
    } else {
        None
    }
}

pub(crate) fn audio_delay_adjustment_row(
    delay_seconds: f64,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
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

    // The entry is the row's source of truth for the current delay: each nudge
    // reads it, applies the sum, and writes the clamped result straight back.
    // This keeps rapid taps accumulating correctly and the readout live without
    // waiting on the pump snapshot to catch up after the set.
    let quick = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    quick.set_halign(gtk::Align::End);
    for (text, delta) in [("-50", -0.05), ("+50", 0.05)] {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_toast = Rc::clone(status_toast);
        let button_entry = entry.clone();
        button.connect_clicked(move |_| {
            // Reject unparsable text rather than treating it as zero, so a nudge
            // never silently replaces invalid input with a real delay.
            let Some(current) = entry_delay_seconds(&button_entry) else {
                mark_delay_entry_error(&button_entry);
                return;
            };
            if let Some(applied) = apply_audio_delay(&button_state, &button_toast, current + delta)
            {
                button_entry.set_text(&subtitle_delay::format_entry(applied));
            }
        });
        quick.append(&button);
    }
    row.append(&quick);

    let apply_state = Rc::clone(state);
    let apply_toast = Rc::clone(status_toast);
    let apply_entry = entry.clone();
    apply_button.connect_clicked(move |_| {
        apply_audio_delay_entry(&apply_entry, Rc::clone(&apply_state), &apply_toast);
    });

    let activate_state = Rc::clone(state);
    let activate_toast = Rc::clone(status_toast);
    entry.connect_activate(move |entry| {
        apply_audio_delay_entry(entry, Rc::clone(&activate_state), &activate_toast);
    });

    let reset_state = Rc::clone(state);
    let reset_toast = Rc::clone(status_toast);
    let reset_entry = entry.clone();
    reset_button.connect_clicked(move |_| {
        if let Some(applied) = apply_audio_delay(&reset_state, &reset_toast, 0.0) {
            reset_entry.set_text(&subtitle_delay::format_entry(applied));
        }
    });

    entry.connect_changed(|entry| {
        entry.remove_css_class("is-error");
    });

    row
}

/// The delay the entry currently spells out, in seconds, or `None` when the
/// text is not a valid delay. Callers reject invalid input the same way Apply
/// does instead of substituting a value.
pub(crate) fn entry_delay_seconds(entry: &gtk::Entry) -> Option<f64> {
    subtitle_delay::parse_entry_seconds(entry.text().as_str())
}

/// Flag a delay entry as rejected: mark it errored and pull focus back so the
/// user can correct the text.
pub(crate) fn mark_delay_entry_error(entry: &gtk::Entry) {
    entry.add_css_class("is-error");
    entry.grab_focus();
}

pub(crate) fn apply_audio_delay_entry(
    entry: &gtk::Entry,
    state: Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
) {
    let Some(delay_seconds) = entry_delay_seconds(entry) else {
        mark_delay_entry_error(entry);
        return;
    };

    if let Some(applied) = apply_audio_delay(&state, status_toast, delay_seconds) {
        // Normalize the field to the stored/clamped whole-millisecond value.
        entry.set_text(&subtitle_delay::format_entry(applied));
    }
}

pub(crate) fn read_tracks(state: &Rc<RefCell<PlayerState>>) -> Vec<Track> {
    let state = state.borrow();
    state
        .mpv
        .as_ref()
        .map(Mpv::observed_tracks)
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubtitleSearchSource {
    Available(PathBuf),
    NoActiveTrack,
    NotExternal,
    UnsupportedFormat,
    MissingPath,
}

impl SubtitleSearchSource {
    fn message(&self) -> &'static str {
        match self {
            Self::Available(_) => "Search the selected external SRT/LRC subtitle track",
            Self::NoActiveTrack => "Select a subtitle track to search",
            Self::NotExternal => "Search supports external SRT/LRC subtitle files",
            Self::UnsupportedFormat => "Search supports SRT and LRC subtitle files",
            Self::MissingPath => "Subtitle file path is unavailable",
        }
    }
}

pub(crate) fn selected_subtitle_search_source(
    tracks: &[Track],
    secondary_subtitle_id: Option<i64>,
    current_file: Option<&Path>,
) -> SubtitleSearchSource {
    let Some(track) = tracks.iter().find(|track| {
        track.kind == TrackKind::Subtitle
            && okp_core::subtitle_tracks::is_primary_subtitle(
                track.id,
                track.selected,
                secondary_subtitle_id,
            )
    }) else {
        return SubtitleSearchSource::NoActiveTrack;
    };

    if !track.external {
        return SubtitleSearchSource::NotExternal;
    }

    let Some(path) = track
        .external_filename
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .and_then(|path| resolve_external_subtitle_path(path, current_file))
    else {
        return SubtitleSearchSource::MissingPath;
    };

    if subtitle_search::is_supported_subtitle_path(&path) {
        SubtitleSearchSource::Available(path)
    } else {
        SubtitleSearchSource::UnsupportedFormat
    }
}

fn resolve_external_subtitle_path(path: &str, current_file: Option<&Path>) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return Some(path);
    }

    let media_dir = current_file?.parent()?;
    if !media_dir.is_absolute() {
        return None;
    }
    Some(media_dir.join(path))
}

pub(crate) fn open_subtitle_search_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let (current_file, source_generation) = {
        let state = state.borrow();
        (state.current_file.clone(), state.source_generation)
    };
    let source = selected_subtitle_search_source(
        &read_tracks(&state),
        read_secondary_subtitle_id(&state),
        current_file.as_deref(),
    );
    let SubtitleSearchSource::Available(path) = source else {
        status_toast.show(source.message());
        return;
    };

    status_toast.show("Loading subtitle search");
    let expected_path = path.clone();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = load_subtitle_search_index(path);
        let _ = sender.send(result);
    });

    let parent = parent.clone();
    glib::timeout_add_local(Duration::from_millis(40), move || {
        match receiver.try_recv() {
            Ok(Ok(index)) => {
                let current_file = {
                    let current = state.borrow();
                    if current.source_generation != source_generation {
                        status_toast.show("Subtitle track changed while loading");
                        return glib::ControlFlow::Break;
                    }
                    current.current_file.clone()
                };
                let current_source = selected_subtitle_search_source(
                    &read_tracks(&state),
                    read_secondary_subtitle_id(&state),
                    current_file.as_deref(),
                );
                if current_source != SubtitleSearchSource::Available(expected_path.clone()) {
                    status_toast.show("Subtitle track changed while loading");
                    return glib::ControlFlow::Break;
                }

                show_subtitle_search_dialog(
                    &parent,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                    index,
                );
                glib::ControlFlow::Break
            }
            Ok(Err(SubtitleSearchLoadError::ReadFailed { path, error })) => {
                eprintln!("Failed to read subtitle file '{}': {error}", path.display());
                status_toast.show("Could not read subtitle file");
                glib::ControlFlow::Break
            }
            Ok(Err(SubtitleSearchLoadError::UnsupportedFormat)) => {
                status_toast.show("Search supports SRT and LRC subtitle files");
                glib::ControlFlow::Break
            }
            Ok(Err(SubtitleSearchLoadError::Empty)) => {
                status_toast.show("No searchable subtitle cues");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                status_toast.show("Could not read subtitle file");
                glib::ControlFlow::Break
            }
        }
    });
}

#[derive(Debug)]
enum SubtitleSearchLoadError {
    ReadFailed {
        path: PathBuf,
        error: std::io::Error,
    },
    UnsupportedFormat,
    Empty,
}

fn load_subtitle_search_index(
    path: PathBuf,
) -> Result<subtitle_search::SubtitleCueIndex, SubtitleSearchLoadError> {
    let text = fs::read_to_string(&path).map_err(|error| SubtitleSearchLoadError::ReadFailed {
        path: path.clone(),
        error,
    })?;
    let Some(index) = subtitle_search::SubtitleCueIndex::from_path_text(&path, Some(&text)) else {
        return Err(SubtitleSearchLoadError::UnsupportedFormat);
    };

    if index.is_empty() {
        return Err(SubtitleSearchLoadError::Empty);
    }

    Ok(index)
}

#[allow(deprecated)]
fn show_subtitle_search_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    index: subtitle_search::SubtitleCueIndex,
) {
    let dialog = gtk::Dialog::builder()
        .title("Search Subtitles")
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Close", gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    content.append(&command_dialog_title("Search Subtitles"));

    let entry_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Cue text"));
    entry_row.append(&entry);

    let search_button = gtk::Button::with_label("Search");
    search_button.add_css_class("okp-sub-adjust-button");
    entry_row.append(&search_button);
    content.append(&entry_row);

    let status = gtk::Label::new(Some("Enter subtitle text"));
    status.add_css_class("okp-info-label");
    status.set_xalign(0.0);
    status.set_wrap(true);
    content.append(&status);

    let results = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.append(&results);

    let index = Rc::new(index);
    let render_results = Rc::new({
        let results = results.clone();
        let status = status.clone();
        let state = Rc::clone(&state);
        let status_toast = Rc::clone(&status_toast);
        move |query: &str| {
            clear_subtitle_search_results(&results);
            let matches = index.search(query, 8);
            if query.trim().is_empty() {
                status.set_text("Enter subtitle text");
                return;
            }
            if matches.is_empty() {
                status.set_text("No matching cues");
                return;
            }

            status.set_text(&format!(
                "{} matching cue{}",
                matches.len(),
                plural_s(matches.len())
            ));
            for cue in matches {
                let label = format!("{}  {}", time_code::format(cue.start_seconds), cue.text);
                let button = track_button(&label, false);
                let seek_state = Rc::clone(&state);
                let seek_toast = Rc::clone(&status_toast);
                button.connect_clicked(move |_| {
                    let subtitle_delay = read_subtitle_adjustments(&seek_state).0;
                    let Some(target) =
                        subtitle_search::delayed_cue_seek_target(cue.start_seconds, subtitle_delay)
                    else {
                        seek_toast.show("Could not seek");
                        return;
                    };
                    if seek_to_time(&seek_state, target) {
                        seek_toast.show(&format!("Jumped to {}", time_code::format(target)));
                    } else {
                        seek_toast.show("Could not seek");
                    }
                });
                results.append(&button);
            }
        }
    });

    let button_entry = entry.clone();
    let button_render = Rc::clone(&render_results);
    search_button.connect_clicked(move |_| {
        button_render(button_entry.text().as_str());
    });

    let activate_render = Rc::clone(&render_results);
    entry.connect_activate(move |entry| {
        activate_render(entry.text().as_str());
    });

    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
    entry.grab_focus();
}

pub(crate) fn open_subtitle_search_preview(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let index = subtitle_search::SubtitleCueIndex::from_srt_text(Some(
        "1\n00:00:04,500 --> 00:00:06,000\nThe first matching subtitle line\n\n\
         2\n00:00:12,250 --> 00:00:14,000\nAnother matching cue",
    ));
    show_subtitle_search_dialog(parent, state, status_toast, index);
}

fn clear_subtitle_search_results(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
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
    let check = selection_check_icon();
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

pub(crate) fn audio_track_button(name: &str, detail: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-track-row");
    button.add_css_class("okp-audio-track-row");
    if selected {
        button.add_css_class("is-selected");
    }

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let check = selection_check_icon();
    check.set_opacity(if selected { 1.0 } else { 0.0 });
    row.append(&check);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
    labels.set_hexpand(true);

    let name_label = gtk::Label::new(Some(name));
    name_label.add_css_class("okp-audio-track-name");
    name_label.set_xalign(0.0);
    name_label.set_ellipsize(pango::EllipsizeMode::End);
    labels.append(&name_label);

    if !detail.is_empty() {
        let detail_label = gtk::Label::new(Some(detail));
        detail_label.add_css_class("okp-audio-track-detail");
        detail_label.set_xalign(0.0);
        detail_label.set_ellipsize(pango::EllipsizeMode::End);
        labels.append(&detail_label);
    }

    row.append(&labels);
    button.set_child(Some(&row));
    button
}

pub(crate) fn command_button(text: &str, selected: bool) -> gtk::Button {
    let button = track_button(text, selected);
    button.add_css_class("okp-command-row");
    button
}

pub(crate) fn selection_check_icon() -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.add_css_class("okp-track-check");
    icon.set_content_width(14);
    icon.set_content_height(14);
    icon.set_valign(gtk::Align::Center);
    icon.set_draw_func(|_, cr, width, height| {
        let width = f64::from(width);
        let height = f64::from(height);
        cr.set_source_rgb(0.04, 0.48, 0.45);
        cr.set_line_width(1.8);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        cr.move_to(width * 0.18, height * 0.52);
        cr.line_to(width * 0.42, height * 0.74);
        cr.line_to(width * 0.82, height * 0.28);
        let _ = cr.stroke();
    });
    icon
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

pub(crate) fn preview_tracks(kind: TrackKind) -> Option<Vec<Track>> {
    let state = env::var("OKP_PLAYER_POPOVER_PREVIEW_STATE").ok()?;
    let tracks = match (state.as_str(), kind) {
        ("subtitle-selected", TrackKind::Subtitle) => vec![
            Track {
                id: 1,
                kind,
                selected: true,
                external: false,
                external_filename: None,
                default: true,
                title: Some("English".to_owned()),
                lang: Some("eng".to_owned()),
                codec: Some("ass".to_owned()),
                audio_channels: None,
            },
            Track {
                id: 2,
                kind,
                selected: false,
                external: true,
                external_filename: Some("Episode 1.en.srt".to_owned()),
                default: false,
                title: Some("English SDH".to_owned()),
                lang: Some("eng".to_owned()),
                codec: Some("subrip".to_owned()),
                audio_channels: None,
            },
        ],
        ("subtitle-empty", TrackKind::Subtitle) => Vec::new(),
        ("subtitle-searchable", TrackKind::Subtitle) => vec![Track {
            id: 2,
            kind,
            selected: true,
            external: true,
            external_filename: Some("subtest.srt".to_owned()),
            default: false,
            title: Some("English SDH".to_owned()),
            lang: Some("eng".to_owned()),
            codec: Some("subrip".to_owned()),
            audio_channels: None,
        }],
        ("audio-selected", TrackKind::Audio) => vec![
            Track {
                id: 1,
                kind,
                selected: true,
                external: false,
                external_filename: None,
                default: true,
                title: Some("English".to_owned()),
                lang: Some("eng".to_owned()),
                codec: Some("eac3".to_owned()),
                audio_channels: Some("5.1".to_owned()),
            },
            Track {
                id: 2,
                kind,
                selected: false,
                external: false,
                external_filename: None,
                default: false,
                title: Some("Director's Commentary".to_owned()),
                lang: Some("eng".to_owned()),
                codec: Some("aac".to_owned()),
                audio_channels: Some("2.0".to_owned()),
            },
        ],
        ("audio-empty", TrackKind::Audio) => Vec::new(),
        _ => return None,
    };
    Some(tracks)
}

/// The one-line descriptor for a track row. Selection is shown by the row's
/// leading check (see [`track_button`]), so the text carries only the track's
/// name and its format tags — no "On" prefix that would shift long titles. The
/// composition itself is portable domain logic and lives in
/// [`okp_core::track_label`] (freeze-boundary); the shell only wires the string.
pub(crate) fn track_label(track: &Track) -> String {
    if track.kind == TrackKind::Audio {
        return okp_core::track_label::audio_track_label(
            track.id,
            track.title.as_deref(),
            track.lang.as_deref(),
            track.audio_channels.as_deref(),
            track.codec.as_deref(),
        );
    }

    okp_core::track_label::subtitle_track_label(
        track.id,
        track.title.as_deref(),
        track.lang.as_deref(),
        track.codec.as_deref(),
        track.external,
        track.default,
    )
}

pub(crate) fn track_base_label(track: &Track) -> String {
    okp_core::track_label::primary_track_name(
        track.id,
        track.title.as_deref(),
        track.lang.as_deref(),
    )
}

pub(crate) fn drain_mpv_events(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) -> Option<VideoDimensions> {
    let events = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(Mpv::take_lifecycle_events)
            .unwrap_or_default()
    };

    let mut auto_fit_dimensions = None;
    for event in events {
        match event {
            MpvEvent::FileLoaded { video_dimensions } => {
                auto_fit_dimensions = auto_fit_dimensions.or(video_dimensions);
                try_pending_audio_device_restore(state);
                try_pending_playback_preferences(state);
                // Companion launch hints win over remembered track preferences for this open only.
                try_pending_launch_tracks(state);
                // A frame is up — the source is playing, not loading anymore.
                let mut state = state.borrow_mut();
                state.media_load_state = network_media::MediaLoadState::Playing;
                state.last_load_error = None;
            }
            MpvEvent::VideoReconfig { video_dimensions } => {
                auto_fit_dimensions = auto_fit_dimensions.or(video_dimensions);
            }
            MpvEvent::EndFile { reason, .. } if reason.is_eof() => {
                if state.borrow().playlist.repeat() != RepeatMode::One {
                    save_current_progress(state, true);
                }
                advance_playlist_on_eof(state);
            }
            MpvEvent::EndFile {
                reason: EndFileReason::Error(error),
                path,
            } => {
                // The engine rejected the source (e.g. a 404 stream). Transition the
                // transport surface to `Failed` and store the short reason for the Copy
                // details action on the in-canvas card. A local-file error is surfaced
                // too, with URL Retry disabled. The
                // staleness guard (drop an error whose source was superseded) lives in
                // `apply_endfile_error` so it is unit-testable without an engine.
                apply_endfile_error(state, error, path.as_deref());
            }
            MpvEvent::CommandReply { request_id, error } => {
                complete_screenshot_capture(state, status_toast, request_id, error);
            }
            _ => {}
        }
    }

    auto_fit_dimensions
}
