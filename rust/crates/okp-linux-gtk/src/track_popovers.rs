use super::*;

pub(crate) const SPEED_POPOVER_WIDTH: i32 = 120;
pub(crate) const SUBTITLE_POPOVER_WIDTH: i32 = 262;
pub(crate) const AUDIO_POPOVER_WIDTH: i32 = 248;
pub(crate) const COMMAND_POPOVER_WIDTH: i32 =
    player_commands::PLAYER_COMMAND_SURFACE_PREFERRED_WIDTH;

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
            Self::More | Self::AdvancedCommands => COMMAND_POPOVER_WIDTH,
        }
    }

    pub(crate) const fn css_class(self) -> &'static str {
        match self {
            Self::Speed => "okp-speed-popover",
            Self::Subtitles => "okp-subtitle-popover",
            Self::Audio => "okp-audio-popover",
            Self::More | Self::AdvancedCommands => "okp-command-popover",
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
    let preset_applicability =
        primary_subtitle_preset_applicability(&tracks, secondary_subtitle_id);

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
    content.append(&subtitle_preset_status_label(preset_applicability));
    content.append(&compact_subtitle_style_row(&state, preset_applicability));
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

/// Existing OSC controls reused by the canonical command dispatcher so both
/// command surfaces invoke the exact same handlers as their direct controls.
#[derive(Clone)]
pub(crate) struct PlayerCommandReach {
    pub(crate) screenshot: gtk::Button,
    pub(crate) fullscreen: gtk::Button,
    pub(crate) chapters: gtk::Button,
    pub(crate) window_bounds: Rc<RefCell<Option<PlayerWindowBounds>>>,
}

pub(crate) fn populate_command_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    reach: &PlayerCommandReach,
    surface: PlayerCommandSurface,
) {
    if idle_theme_is_dark() {
        popover.add_css_class("is-dark");
    } else {
        popover.remove_css_class("is-dark");
    }
    let commands = resolved_player_commands(surface, parent, &state);
    let content = searchable_player_command_content(
        popover,
        parent,
        state,
        status_toast,
        reach,
        surface,
        commands,
    );
    set_player_command_popover_child(popover, parent, &reach.window_bounds, surface, content);
}

pub(crate) fn resolved_player_commands(
    surface: PlayerCommandSurface,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> Vec<ResolvedPlayerCommand> {
    let (mut context, bindings) = {
        let state = state.borrow();
        let bindings = resolved_shortcut_bindings(&state.settings).unwrap_or_else(|error| {
            eprintln!(
                "Ignoring custom keybindings at line {}: {}",
                error.line, error.message
            );
            shortcuts::default_bindings()
        });
        (
            PlayerCommandContext {
                has_media: has_loaded_media_state(&state),
                has_local_media: state.current_file.is_some(),
                has_video_geometry: state.current_video_dimensions.is_some(),
                playlist_count: state.playlist.len(),
                repeat_mode: state.playlist.repeat(),
                shuffle_enabled: state.playlist.shuffle(),
                auto_advance_enabled: state.playlist.auto_advance(),
                private_session: state.private_session,
                ab_loop_active: state.ab_loop.is_active(),
                compact_mode: window_compact_mode_active(parent),
                fullscreen: parent.is_fullscreen(),
                video_geometry: state.video_transform,
            },
            bindings,
        )
    };
    if env::var_os("OKP_PLAYER_POPOVER_PREVIEW_STATE")
        .is_some_and(|state| state.eq_ignore_ascii_case("more-disabled"))
    {
        context.has_media = false;
        context.has_local_media = false;
        context.has_video_geometry = false;
        context.playlist_count = 0;
    }

    player_commands::resolve_player_commands(surface, context, |action| {
        let labels = shortcuts::chords_for_action(&bindings, action)
            .into_iter()
            .map(|chord| chord.label())
            .collect::<Vec<_>>();
        (!labels.is_empty()).then(|| labels.join(" / "))
    })
}

fn searchable_player_command_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    reach: &PlayerCommandReach,
    surface: PlayerCommandSurface,
    commands: Vec<ResolvedPlayerCommand>,
) -> gtk::Box {
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("okp-command-surface");

    let search = gtk::SearchEntry::new();
    search.add_css_class("okp-command-search");
    search.set_placeholder_text(Some("Search commands"));
    search.set_accessible_role(gtk::AccessibleRole::SearchBox);
    shell.append(&search);

    let rows_host = gtk::Box::new(gtk::Orientation::Vertical, 2);
    rows_host.add_css_class("okp-command-results");
    let focus_rows = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));

    let render = {
        let rows_host = rows_host.clone();
        let search = search.clone();
        let commands = Rc::new(commands);
        let focus_rows = Rc::clone(&focus_rows);
        let popover = popover.clone();
        let parent = parent.clone();
        let state = Rc::clone(&state);
        let status_toast = Rc::clone(&status_toast);
        let reach = reach.clone();
        Rc::new(move |query: &str| {
            render_player_command_rows(
                &rows_host,
                &search,
                &commands,
                query,
                &focus_rows,
                &popover,
                &parent,
                Rc::clone(&state),
                Rc::clone(&status_toast),
                &reach,
                surface,
            );
        })
    };

    let render_changed = Rc::clone(&render);
    search.connect_search_changed(move |entry| render_changed(entry.text().as_str()));
    render("");
    if let Ok(query) = env::var("OKP_COMMAND_SEARCH_QUERY") {
        search.set_text(&query);
    }

    let activate_rows = Rc::clone(&focus_rows);
    search.connect_activate(move |_| {
        if let Some(button) = activate_rows
            .borrow()
            .iter()
            .find(|button| button.is_sensitive())
        {
            button.emit_clicked();
        }
    });

    let key_rows = Rc::clone(&focus_rows);
    let key_popover = popover.clone();
    let key_search = search.clone();
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, _| match key {
        gdk::Key::Escape => {
            if key_search.text().is_empty() {
                key_popover.popdown();
            } else {
                key_search.set_text("");
                key_search.grab_focus();
            }
            glib::Propagation::Stop
        }
        gdk::Key::Down if key_search.has_focus() => {
            if let Some(button) = key_rows
                .borrow()
                .iter()
                .find(|button| button.is_sensitive())
            {
                button.grab_focus();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
        _ => glib::Propagation::Proceed,
    });
    shell.add_controller(keys);

    let search_focus = search.clone();
    glib::idle_add_local_once(move || {
        search_focus.grab_focus();
    });

    shell.append(&rows_host);
    shell
}

#[allow(clippy::too_many_arguments)]
fn render_player_command_rows(
    rows_host: &gtk::Box,
    search: &gtk::SearchEntry,
    commands: &[ResolvedPlayerCommand],
    query: &str,
    focus_rows: &Rc<RefCell<Vec<gtk::Button>>>,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    reach: &PlayerCommandReach,
    surface: PlayerCommandSurface,
) {
    while let Some(child) = rows_host.first_child() {
        rows_host.remove(&child);
    }
    focus_rows.borrow_mut().clear();

    let filtered = player_commands::filter_player_commands(commands, query);
    if filtered.is_empty() {
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 4);
        empty.add_css_class("okp-command-no-results");
        let title = gtk::Label::new(Some("No commands found"));
        title.add_css_class("okp-command-no-results-title");
        let hint = gtk::Label::new(Some("Try a command name or related term."));
        hint.add_css_class("okp-command-no-results-hint");
        empty.append(&title);
        empty.append(&hint);
        rows_host.append(&empty);
        return;
    }

    let mut current_group = None;
    for command in filtered {
        if current_group != Some(command.group) {
            current_group = Some(command.group);
            rows_host.append(&player_command_group_title(command.group));
        }

        let button = player_command_button(&command);
        if command.id == PlayerCommandId::ExportClip {
            button.set_tooltip_text(Some(&clip_export_placeholder_reason(
                current_clip_export_eligibility(&state),
            )));
        }
        let activate_popover = popover.clone();
        let activate_parent = parent.clone();
        let activate_state = Rc::clone(&state);
        let activate_toast = Rc::clone(&status_toast);
        let activate_reach = reach.clone();
        let id = command.id;
        button.connect_clicked(move |_| {
            player_commands::dispatch_player_command(surface, id, |id| {
                dispatch_player_command_action(
                    id,
                    &activate_popover,
                    &activate_parent,
                    &activate_state,
                    &activate_toast,
                    &activate_reach,
                );
            });
        });
        rows_host.append(&button);
        focus_rows.borrow_mut().push(button);
    }

    let row_snapshot = focus_rows.borrow().clone();
    for (index, button) in row_snapshot.iter().enumerate() {
        let navigation_rows = Rc::clone(focus_rows);
        let navigation_search = search.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            let rows = navigation_rows.borrow();
            let target = match key {
                gdk::Key::Down => rows
                    .iter()
                    .skip(index + 1)
                    .find(|button| button.is_sensitive())
                    .cloned(),
                gdk::Key::Up => rows[..index]
                    .iter()
                    .rev()
                    .find(|button| button.is_sensitive())
                    .cloned(),
                _ => return glib::Propagation::Proceed,
            };
            if let Some(target) = target {
                target.grab_focus();
            } else if key == gdk::Key::Up {
                navigation_search.grab_focus();
            }
            glib::Propagation::Stop
        });
        button.add_controller(keys);
    }
}

fn player_command_group_title(group: PlayerCommandGroup) -> gtk::Label {
    let label = gtk::Label::new(Some(group.label()));
    label.add_css_class("okp-command-group-title");
    label.set_xalign(0.0);
    label
}

fn player_command_button(command: &ResolvedPlayerCommand) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-command-row");
    button.set_sensitive(command.enabled);
    if command.checked {
        button.add_css_class("is-selected");
    }

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let check = selection_check_icon();
    check.set_opacity(if command.checked { 1.0 } else { 0.0 });
    row.append(&check);

    let label = gtk::Label::new(Some(&command.label));
    label.add_css_class("okp-command-row-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);
    row.append(&label);

    if let Some(shortcut) = &command.shortcut {
        let shortcut = gtk::Label::new(Some(shortcut));
        shortcut.add_css_class("okp-command-shortcut");
        shortcut.set_xalign(1.0);
        row.append(&shortcut);
    }

    button.set_child(Some(&row));
    button.update_property(&[gtk::accessible::Property::Label(&command.label)]);
    button
}

fn set_player_command_popover_child(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    surface: PlayerCommandSurface,
    content: gtk::Box,
) {
    let work_area = current_player_work_area(parent, reported_bounds).or_else(|| {
        parent.surface().and_then(|surface| {
            let monitor = surface.display().monitor_at_surface(&surface)?;
            let geometry = monitor.geometry();
            Some(window_fit::WindowRect {
                x: geometry.x(),
                y: geometry.y(),
                width: geometry.width(),
                height: geometry.height(),
            })
        })
    });
    let size = work_area
        .map(|area| player_commands::player_command_surface_size(area.width, area.height))
        .unwrap_or(window_fit::WindowSize {
            width: COMMAND_POPOVER_WIDTH,
            height: 520,
        });

    let height = if surface == PlayerCommandSurface::ContextMenu {
        size.height.min(400)
    } else {
        size.height
    };
    content.set_width_request(size.width);
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("okp-command-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_width(size.width);
    scroll.set_max_content_width(size.width);
    scroll.set_max_content_height((height - 50).max(1));
    scroll.set_propagate_natural_height(true);

    let shell = content;
    let rows = shell
        .last_child()
        .expect("command shell always contains the results host");
    shell.remove(&rows);
    scroll.set_child(Some(&rows));
    shell.append(&scroll);
    popover.set_child(Some(&shell));
}

pub(crate) fn dispatch_player_command_action(
    id: PlayerCommandId,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
    reach: &PlayerCommandReach,
) {
    use PlayerCommandId as Id;

    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!("interaction: player-command={}", id.id());
    }

    if let Some(action) = player_commands::geometry_action(id) {
        popover.popdown();
        apply_video_geometry_action(state, action, status_toast);
        return;
    }

    match id {
        Id::GoToTime => {
            popover.popdown();
            open_go_to_time_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::CopyCurrentTime => {
            popover.popdown();
            copy_current_time(state, status_toast);
        }
        Id::AddBookmark => {
            popover.popdown();
            add_bookmark_at_position(state, status_toast);
        }
        Id::AbLoop => {
            popover.popdown();
            toggle_ab_loop(state, status_toast);
        }
        Id::RepeatMode => {
            popover.popdown();
            cycle_repeat_mode(state, status_toast);
        }
        Id::Shuffle => {
            popover.popdown();
            toggle_shuffle(state, status_toast);
        }
        Id::AutoAdvance => {
            popover.popdown();
            toggle_auto_advance(state, status_toast);
        }
        Id::PlaybackSpeed => populate_speed_popover(popover, Rc::clone(state)),
        Id::Subtitles => {
            populate_subtitle_popover(popover, parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::AudioTrack => populate_audio_popover(popover, Rc::clone(state)),
        Id::ChaptersUpNext => {
            popover.popdown();
            reach.chapters.emit_clicked();
        }
        Id::FitWindowToMedia => {
            popover.popdown();
            fit_player_window_to_current_media(parent, state, &reach.window_bounds, status_toast);
        }
        Id::MiniPlayer => {
            popover.popdown();
            toggle_compact_mode(parent);
        }
        Id::Fullscreen => {
            popover.popdown();
            reach.fullscreen.emit_clicked();
        }
        Id::OpenFile => {
            popover.popdown();
            open_media_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::OpenUrl => {
            popover.popdown();
            open_url_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::OpenFolder => {
            popover.popdown();
            open_folder_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::OpenPlaylist => {
            popover.popdown();
            open_playlist_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::AddToQueue => {
            popover.popdown();
            open_queue_media_dialog(
                parent,
                Rc::clone(state),
                Rc::clone(status_toast),
                QueueInsertMode::Append,
            );
        }
        Id::PlayNext => {
            popover.popdown();
            open_queue_media_dialog(
                parent,
                Rc::clone(state),
                Rc::clone(status_toast),
                QueueInsertMode::PlayNext,
            );
        }
        Id::SavePlaylist => {
            popover.popdown();
            save_playlist_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::CloseMedia => {
            popover.popdown();
            close_current_media(state, status_toast);
        }
        Id::MediaInfo => {
            popover.popdown();
            open_media_info_window(parent, state, Rc::clone(status_toast));
        }
        Id::OpenFileLocation => {
            popover.popdown();
            open_current_file_location(state, status_toast);
        }
        Id::SaveFrame => {
            popover.popdown();
            reach.screenshot.emit_clicked();
        }
        Id::SaveFrameWithSubtitles => {
            popover.popdown();
            save_screenshot(state, status_toast, true);
        }
        Id::CopyFrame => {
            popover.popdown();
            copy_frame_to_clipboard(state, status_toast);
        }
        Id::ExportClip => {}
        Id::PrivateSession => {
            popover.popdown();
            toggle_private_session(state, status_toast);
        }
        Id::ClearHistory => {
            popover.popdown();
            open_clear_history_dialog(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::OpenSettings => {
            popover.popdown();
            open_settings_window(parent, Rc::clone(state), Rc::clone(status_toast));
        }
        Id::AspectAuto
        | Id::AspectWide
        | Id::AspectStandard
        | Id::AspectCinema
        | Id::ZoomIn
        | Id::ZoomOut
        | Id::PanLeft
        | Id::PanRight
        | Id::PanUp
        | Id::PanDown
        | Id::CenterImage
        | Id::RotateClockwise
        | Id::FillScreen
        | Id::Deinterlace
        | Id::ResetVideo => unreachable!("video geometry commands dispatch above"),
    }
}

pub(crate) fn current_clip_export_eligibility(
    state: &Rc<RefCell<PlayerState>>,
) -> ClipExportEligibility {
    if let Some(preview) = clip_export_preview_eligibility() {
        return preview;
    }

    let ab_loop = state.borrow().ab_loop;
    let tooling = if find_executable("ffmpeg").is_some() {
        ClipExportTooling::Available
    } else {
        ClipExportTooling::MissingFfmpeg
    };
    clip_export::clip_export_eligibility(ab_loop.a, ab_loop.b, tooling, ClipExportLimits::default())
}

pub(crate) fn clip_export_preview_eligibility() -> Option<ClipExportEligibility> {
    let state = env::var("OKP_CLIP_EXPORT_PREVIEW_STATE").ok()?;
    let limits = ClipExportLimits::default();
    match state.as_str() {
        "no-selection" => Some(ClipExportEligibility::NoSelection),
        "invalid-range" => Some(ClipExportEligibility::InvalidRange),
        "too-short" => Some(ClipExportEligibility::SelectionTooShort {
            duration_seconds: limits.min_seconds / 2.0,
            min_seconds: limits.min_seconds,
        }),
        "too-long" => Some(ClipExportEligibility::SelectionTooLong {
            duration_seconds: limits.max_seconds + 1.0,
            max_seconds: limits.max_seconds,
        }),
        "missing-tooling" => Some(ClipExportEligibility::MissingTooling),
        "ready" => Some(ClipExportEligibility::Ready(
            okp_core::clip_export::ClipExportSelection {
                start_seconds: 12.0,
                end_seconds: 42.0,
            },
        )),
        _ => None,
    }
}

pub(crate) fn clip_export_placeholder_reason(eligibility: ClipExportEligibility) -> String {
    match eligibility {
        ClipExportEligibility::NoSelection => "Set both A and B to prepare export".to_owned(),
        ClipExportEligibility::InvalidRange => "Set B after A".to_owned(),
        ClipExportEligibility::SelectionTooShort { min_seconds, .. } => {
            format!("Select at least {}", export_limit_label(min_seconds))
        }
        ClipExportEligibility::SelectionTooLong { max_seconds, .. } => {
            format!("Select {} or less", export_limit_label(max_seconds))
        }
        ClipExportEligibility::MissingTooling => "Install FFmpeg to export".to_owned(),
        ClipExportEligibility::Ready(selection) => format!(
            "Selection ready ({}); encoder not enabled",
            time_code::format_clock(selection.duration_seconds())
        ),
    }
}

pub(crate) fn export_limit_label(seconds: f64) -> String {
    if seconds >= 60.0 && seconds % 60.0 == 0.0 {
        let minutes = seconds / 60.0;
        format!(
            "{} {}",
            minutes as u64,
            if minutes == 1.0 { "minute" } else { "minutes" }
        )
    } else {
        format!(
            "{} {}",
            seconds as u64,
            if seconds == 1.0 { "second" } else { "seconds" }
        )
    }
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

pub(crate) fn compact_subtitle_style_row(
    state: &Rc<RefCell<PlayerState>>,
    applicability: okp_core::subtitle_tracks::SubtitlePresetApplicability,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("okp-quick-style-row");

    let label = gtk::Label::new(Some("Style"));
    label.add_css_class("okp-quick-delay-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let current = state.borrow().settings.subtitle_style();
    let button_label = match applicability {
        okp_core::subtitle_tracks::SubtitlePresetApplicability::Applies(_) => {
            format!("{}  ›", subtitle_style_label(current.key))
        }
        okp_core::subtitle_tracks::SubtitlePresetApplicability::NativeStyle(_) => {
            "Native".to_owned()
        }
        okp_core::subtitle_tracks::SubtitlePresetApplicability::Unsupported(_)
        | okp_core::subtitle_tracks::SubtitlePresetApplicability::NoActiveTrack => {
            "Unavailable".to_owned()
        }
    };
    let button = gtk::Button::with_label(&button_label);
    button.add_css_class("okp-quick-style-button");
    button.set_sensitive(matches!(
        applicability,
        okp_core::subtitle_tracks::SubtitlePresetApplicability::Applies(_)
    ));
    button.set_tooltip_text(Some(&subtitle_preset_status_text(applicability)));
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

pub(crate) fn subtitle_preset_status_label(
    applicability: okp_core::subtitle_tracks::SubtitlePresetApplicability,
) -> gtk::Label {
    let label = gtk::Label::new(Some(&subtitle_preset_status_text(applicability)));
    label.add_css_class("okp-subtitle-preset-status");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_max_width_chars(34);
    label
}

pub(crate) fn subtitle_preset_status_text(
    applicability: okp_core::subtitle_tracks::SubtitlePresetApplicability,
) -> String {
    use okp_core::subtitle_tracks::{SubtitlePresetApplicability, SubtitlePresetFormat};

    match applicability {
        SubtitlePresetApplicability::Applies(format) => format!(
            "OK Player preset applies to this {} track.",
            format.display_name()
        ),
        SubtitlePresetApplicability::NativeStyle(format) => format!(
            "{} native style; OK Player preset is not applied.",
            format.display_name()
        ),
        SubtitlePresetApplicability::Unsupported(SubtitlePresetFormat::Image) => {
            "Image subtitle; style presets are unavailable.".to_owned()
        }
        SubtitlePresetApplicability::Unsupported(_) => {
            "Style support is unavailable for this subtitle format.".to_owned()
        }
        SubtitlePresetApplicability::NoActiveTrack => {
            "Select a subtitle track to use style presets.".to_owned()
        }
    }
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

pub(crate) fn primary_subtitle_preset_applicability(
    tracks: &[Track],
    secondary_subtitle_id: Option<i64>,
) -> okp_core::subtitle_tracks::SubtitlePresetApplicability {
    okp_core::subtitle_tracks::primary_preset_applicability(
        tracks
            .iter()
            .map(|track| okp_core::subtitle_tracks::SubtitleTrackMetadata {
                id: track.id,
                selected: track.selected,
                codec: track.codec.as_deref(),
                external_filename: track.external_filename.as_deref(),
            }),
        secondary_subtitle_id,
    )
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
            Track {
                id: 3,
                kind,
                selected: false,
                external: false,
                external_filename: None,
                default: false,
                title: Some("English Forced".to_owned()),
                lang: Some("eng".to_owned()),
                codec: Some("hdmv_pgs_subtitle".to_owned()),
                audio_channels: None,
            },
        ],
        ("subtitle-empty", TrackKind::Subtitle) => Vec::new(),
        ("subtitle-srt-selected", TrackKind::Subtitle) => vec![Track {
            id: 2,
            kind,
            selected: true,
            external: true,
            external_filename: Some("Episode 1.en.srt".to_owned()),
            default: false,
            title: Some("English SDH".to_owned()),
            lang: Some("eng".to_owned()),
            codec: Some("subrip".to_owned()),
            audio_channels: None,
        }],
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
        track.external_filename.as_deref(),
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
                state.last_load_diagnostic = None;
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
                diagnostic_messages,
            } => {
                // The engine rejected the source (e.g. a 404 stream). Transition the
                // transport surface to `Failed` and store the short reason for the Copy
                // details action on the in-canvas card. A local-file error is surfaced
                // too, with URL Retry disabled. The
                // staleness guard (drop an error whose source was superseded) lives in
                // `apply_endfile_error` so it is unit-testable without an engine.
                apply_endfile_error(state, error, path.as_deref(), &diagnostic_messages);
            }
            MpvEvent::CommandReply { request_id, error } => {
                complete_screenshot_capture(state, status_toast, request_id, error);
            }
            _ => {}
        }
    }

    auto_fit_dimensions
}
