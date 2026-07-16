use super::*;

pub(crate) fn connect_mpv(
    video_area: &gtk::GLArea,
    state: Rc<RefCell<PlayerState>>,
    launch_args: LaunchArgs,
) {
    let realize_state = Rc::clone(&state);
    video_area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            eprintln!("GTK GLArea error: {error}");
            return;
        }

        let (hwdec, raw_mpv_config) = {
            let state = realize_state.borrow();
            (
                state.settings.hardware_decode_mpv_option().to_owned(),
                state.settings.raw_mpv_config().to_owned(),
            )
        };
        let raw_mpv_options = match parse_raw_mpv_config(&raw_mpv_config) {
            Ok(options) => options,
            Err(error) => {
                eprintln!(
                    "Ignoring custom mpv.conf option at line {}: {}",
                    error.line, error.message
                );
                Vec::new()
            }
        };

        let mut mpv = match Mpv::new_with_options(&hwdec, &raw_mpv_options) {
            Ok(mpv) => mpv,
            Err(error) if !raw_mpv_options.is_empty() => {
                eprintln!(
                    "Failed to create mpv with custom mpv.conf options: {error}; retrying without them"
                );
                match Mpv::new_with_hwdec(&hwdec) {
                    Ok(mpv) => mpv,
                    Err(error) => {
                        eprintln!("Failed to create mpv: {error}");
                        return;
                    }
                }
            }
            Err(error) => {
                eprintln!("Failed to create mpv: {error}");
                return;
            }
        };
        // The realize handler runs on the GLib main context: arm the debug
        // tripwire so blocking property reads issued from this thread are
        // hard-logged with a backtrace (the deadlock class from the Windows
        // #33 postmortem). No-op in release builds.
        mpv.mark_ui_thread();
        let saved_volume = realize_state.borrow().settings.volume();
        if let Err(error) = mpv.set_volume(saved_volume) {
            eprintln!("Failed to restore saved volume: {error}");
        }
        let video_adjustments = realize_state.borrow().settings.video_adjustments();
        if let Err(error) = mpv.set_video_adjustments(
            video_adjustments.brightness,
            video_adjustments.contrast,
            video_adjustments.saturation,
            video_adjustments.gamma,
        ) {
            eprintln!("Failed to restore video adjustments: {error}");
        }
        let audio_normalization = realize_state
            .borrow()
            .settings
            .audio_normalization_enabled();
        if let Err(error) = mpv.set_audio_normalization(audio_normalization) {
            eprintln!("Failed to restore audio normalization: {error}");
        }

        if let Err(error) = mpv.create_render_context() {
            eprintln!("Failed to create mpv render context: {error}");
            return;
        }

        // Start the background event pump: from here on the shell reads playback
        // state from its observed snapshot rather than polling mpv from this
        // (GLib main-context) thread, so the tripwire armed above stays green.
        mpv.start_event_pump();

        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);

        apply_launch_args(&realize_state, &launch_args);
    });

    let render_target_size = Rc::new(Cell::new(None));
    let resize_target_size = Rc::clone(&render_target_size);
    video_area.connect_resize(move |area, width, height| {
        let target_size = (width > 0 && height > 0).then_some(RenderTargetSize { width, height });
        resize_target_size.set(target_size);
        if target_size.is_some() {
            area.queue_render();
        }
    });

    let render_state = Rc::clone(&state);
    let render_target_size = Rc::clone(&render_target_size);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let widget_width = area.width();
        let widget_height = area.height();
        let scale_factor = area.scale_factor();
        let mut state = render_state.borrow_mut();
        let target_size = resolve_render_target_size(
            render_target_size.get(),
            widget_width,
            widget_height,
            scale_factor,
        );
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(target_size.width, target_size.height)
        {
            eprintln!("mpv render failed: {error}");
        }

        glib::Propagation::Stop
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |area| {
        area.make_current();
        if let Some(mpv) = unrealize_state.borrow_mut().mpv.as_mut() {
            mpv.destroy_render_context();
        }
    });

    let tick_area = video_area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_area.queue_render();
        glib::ControlFlow::Continue
    });
}

pub(crate) fn parse_raw_mpv_config(text: &str) -> Result<Vec<(String, String)>, RawMpvConfigError> {
    let mut options = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let option = trimmed.strip_prefix("--").unwrap_or(trimmed);
        let Some((name, value)) = option.split_once('=') else {
            return Err(raw_mpv_config_error(
                line_number,
                "Use key=value syntax, one option per line.",
            ));
        };
        let name = name.trim();
        let value = value.trim();

        if name.is_empty() {
            return Err(raw_mpv_config_error(line_number, "Option name is empty."));
        }
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(raw_mpv_config_error(
                line_number,
                "Option names can use letters, numbers, hyphen, or underscore.",
            ));
        }
        if name.contains('\0') || value.contains('\0') {
            return Err(raw_mpv_config_error(
                line_number,
                "NUL bytes are not valid in mpv options.",
            ));
        }
        if PROTECTED_MPV_OPTIONS
            .iter()
            .any(|protected| name.eq_ignore_ascii_case(protected))
        {
            return Err(raw_mpv_config_error(
                line_number,
                &format!("{name} is managed by OK Player."),
            ));
        }

        options.push((name.to_owned(), value.to_owned()));
    }

    Ok(options)
}

pub(crate) fn raw_mpv_config_error(line: usize, message: &str) -> RawMpvConfigError {
    RawMpvConfigError {
        line,
        message: message.to_owned(),
    }
}

pub(crate) fn apply_launch_args(
    state: &Rc<RefCell<PlayerState>>,
    launch_args: &LaunchArgs,
) -> bool {
    if launch_args.has_payload() {
        eprintln!(
            "Launch request: {} item(s), {} playlist(s), {} subtitle(s)",
            launch_args.items.len(),
            launch_args.playlists.len(),
            launch_args.subtitles.len()
        );
    }

    if launch_args.has_media_payload() {
        state.borrow_mut().next_launch_directives = Some(launch_args.directives);
    }

    let loaded = load_launch_args(state, launch_args);
    if !loaded {
        state.borrow_mut().next_launch_directives = None;
    }
    let subtitles_loaded = apply_launch_subtitles(state, &launch_args.subtitles);
    loaded || subtitles_loaded
}

pub(crate) fn load_launch_args(state: &Rc<RefCell<PlayerState>>, launch_args: &LaunchArgs) -> bool {
    match launch_args.items.as_slice() {
        [PlaylistItem::Local(path)] => {
            load_media_path(state, path.clone());
            true
        }
        [PlaylistItem::Url(url)] => {
            load_media_url(state, url.clone());
            true
        }
        [] => launch_args
            .playlists
            .first()
            .is_some_and(|path| load_m3u_playlist_silent(state, path)),
        items => {
            let playlist = items.to_vec();
            let Some(first_item) = playlist.first().cloned() else {
                return false;
            };
            load_playlist_item_with_playlist(state, first_item, playlist, true)
        }
    }
}

pub(crate) fn apply_launch_subtitles(
    state: &Rc<RefCell<PlayerState>>,
    subtitles: &[PathBuf],
) -> bool {
    let mut applied = false;
    for path in subtitles {
        if load_subtitle_path(state, path.clone()) {
            applied = true;
        } else if !has_loaded_media(state) {
            let mut state = state.borrow_mut();
            if !state
                .pending_subtitles
                .iter()
                .any(|existing| existing == path)
            {
                state.pending_subtitles.push(path.clone());
            }
        }
    }
    applied
}

pub(crate) fn connect_state_poll(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    controls: Controls,
    context: StatePollContext,
) {
    let window = window.clone();
    let status_toast = controls.status_toast.clone();
    let StatePollContext {
        updating_seek,
        chrome,
        window_chrome,
        subtitle_position_snapshot,
        empty_surface,
        lyrics_surface,
        media_state_overlay,
        window_bounds,
        mpris_snapshot,
        mpris_signals,
    } = context;
    let last_auto_fit_generation = Cell::new(None);
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let auto_fit_dimensions = drain_mpv_events(&state, &status_toast);
        if let Some(dimensions) = auto_fit_dimensions {
            let generation = state.borrow().source_generation;
            if last_auto_fit_generation.replace(Some(generation)) != Some(generation) {
                fit_player_window_to_video(&window, &state, &window_bounds, generation, dimensions);
            }
        }
        drain_screenshot_jobs(&state, &status_toast);
        try_pending_audio_device_restore(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_playback_state());
        let has_media = has_loaded_media(&state);
        let seek_preview = env::var_os("OKP_OPEN_SEEK_PREVIEW_ON_STARTUP").is_some();
        let has_chapters = state
            .borrow()
            .mpv
            .as_ref()
            .is_some_and(|mpv| !mpv.observed_chapters().is_empty());
        chrome.set_has_media(has_media || seek_preview);
        let media_title = if has_media {
            let state = state.borrow();
            let base = state
                .mpv
                .as_ref()
                .and_then(Mpv::observed_media_info)
                .map(|info| info.title)
                .filter(|title| !title.trim().is_empty())
                .or_else(|| {
                    state
                        .current_file
                        .as_ref()
                        .map(|path| PlaylistItem::Local(path.clone()).display_name())
                })
                .or_else(|| {
                    state
                        .current_url
                        .as_ref()
                        .map(|url| PlaylistItem::Url(url.clone()).display_name())
                })
                .unwrap_or_default();
            let chapter = playback
                .and_then(|playback| playback.time_pos)
                .and_then(|position| {
                    let times = state
                        .chapters_snapshot
                        .iter()
                        .map(|chapter| chapter.time)
                        .collect::<Vec<_>>();
                    chapter_math::current_index(&times, position, chapter_math::DEFAULT_EPSILON)
                        .and_then(|index| state.chapters_snapshot.get(index))
                })
                .and_then(|chapter| chapter.title.as_deref())
                .filter(|title| !title.trim().is_empty());
            chapter
                .map(|chapter| format!("{base} · {chapter}"))
                .unwrap_or(base)
        } else {
            String::new()
        };
        window_chrome.set_title(&media_title);
        if has_media {
            let lift = if chrome.is_revealed() {
                okp_core::subtitle_lift::for_surface(
                    f64::from(window.height()),
                    OSC_CLEARANCE_DIP,
                    OSC_SUBTITLE_LIFT_PERCENT,
                )
            } else {
                0.0
            };
            let subtitle_position = (100.0 - lift).clamp(0.0, 100.0);
            let position_key = (subtitle_position * 1000.0).round() as i64;
            if subtitle_position_snapshot.replace(Some(position_key)) != Some(position_key)
                && let Some(mpv) = state.borrow().mpv.as_ref()
                && let Err(error) = mpv.set_subtitle_position(subtitle_position)
            {
                eprintln!("Failed to position subtitles above playback chrome: {error}");
            }
        } else {
            subtitle_position_snapshot.set(None);
        }
        {
            let state = state.borrow();
            update_mpris_snapshot(&mpris_snapshot, &mpris_signals, &state, playback);
        }
        sync_ab_loop_state(&state, has_media);
        if has_media {
            empty_surface.clear_preview_substrate();
        }
        // Hide the welcome surface behind an active lyrics preview so the fixture reads cleanly;
        // in production the loaded audio already hides it (`is_preview_frozen` stays false).
        empty_surface.refresh(&window, &state, Rc::clone(&status_toast));
        let failed = state.borrow().media_load_state == network_media::MediaLoadState::Failed;
        empty_surface.set_has_media(has_media || failed || lyrics_surface.is_preview_frozen());
        lyrics_surface.update(&state);
        drain_thumbnail_events(&controls);
        update_up_next_panel(&controls, &state, &chrome);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);
            let load_state = state.borrow().media_load_state;
            chrome.set_auto_hide_enabled(
                has_media
                    && load_state == network_media::MediaLoadState::Playing
                    && !playback.paused,
            );

            let duration = playback.duration.unwrap_or(0.0).max(0.0);
            let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
            let time_pos = if duration > 0.0 {
                raw_time.min(duration)
            } else {
                raw_time
            };
            try_pending_resume(&state, duration);

            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_chapters);
            controls.next_button.set_sensitive(has_chapters);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls.play_button.set_icon_name(if playback.paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            });
            controls
                .play_button
                .set_tooltip_text(Some(if playback.paused {
                    "Play (Space)"
                } else {
                    "Pause (Space)"
                }));
            controls
                .speed_button
                .set_label(&format_speed(playback.speed.unwrap_or(1.0)));
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(has_media && duration > 0.0);

            updating_seek.set(true);
            controls.seek.set_range(0.0, duration.max(1.0));
            controls.seek.set_value(time_pos);
            updating_seek.set(false);

            if load_state == network_media::MediaLoadState::Loading {
                controls.timeline_rail.set_loading(true);
                controls.timeline_rail.pulse();
            } else {
                controls.timeline_rail.set_loading(false);
                let fraction = timeline_buffer::fraction(
                    playback.time_pos,
                    playback.cache_duration,
                    playback.duration,
                );
                controls.timeline_rail.set_buffered_fraction(fraction);
            }

            if let Some(volume) = playback.volume {
                controls.volume.sync_level(volume);
            }

            controls
                .elapsed_label
                .set_text(&time_code::format_clock(time_pos));
            // Unknown duration shows the live `--:--` sentinel only for a network source;
            // local loading remains `-00:00`. The pure core helper owns that distinction
            // and the remaining-time clamp so the shell only projects the value.
            // The seek range still clamps to 0 so the bar stays progress-only /
            // disabled rather than running broken timeline math.
            let is_url = state.borrow().current_url.is_some();
            controls
                .duration_label
                .set_text(&time_code::format_trailing(
                    controls.trailing_time_mode.get(),
                    is_url,
                    time_pos,
                    playback.duration,
                ));
        } else {
            chrome.set_auto_hide_enabled(false);
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_chapters);
            controls.next_button.set_sensitive(has_chapters);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls
                .play_button
                .set_icon_name("media-playback-start-symbolic");
            controls.play_button.set_tooltip_text(Some("Play (Space)"));
            controls.speed_button.set_label("1.00×");
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(false);
            updating_seek.set(true);
            controls.seek.set_range(0.0, 1.0);
            controls.seek.set_value(0.0);
            updating_seek.set(false);
            controls.timeline_rail.set_buffered_fraction(0.0);
            controls.timeline_rail.set_loading(false);
            controls.elapsed_label.set_text("00:00");
            controls.duration_label.set_text("-00:00");
        }

        update_media_state_surface(&state, playback, has_media, &media_state_overlay);

        glib::ControlFlow::Continue
    });
}

/// Project the shared load state and observed pause flag onto the in-canvas
/// paused, loading, and recovery surfaces. Raw engine detail stays behind the
/// error card's explicit Copy details action.
fn update_media_state_surface(
    state: &Rc<RefCell<PlayerState>>,
    playback: Option<PlaybackState>,
    has_media: bool,
    overlay: &MediaStateOverlay,
) {
    let (load_state, can_retry) = {
        let state = state.borrow();
        (state.media_load_state, state.retry_load_source.is_some())
    };
    overlay.update(
        load_state,
        has_media,
        playback.is_some_and(|playback| playback.paused),
        can_retry,
    );
}

pub(crate) fn connect_video_clicks(
    video_area: &gtk::GLArea,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);

    let click_window = window.clone();
    let click_state = Rc::clone(&state);
    let pending_single_click = Rc::new(RefCell::new(None::<glib::SourceId>));
    let pending_click = Rc::clone(&pending_single_click);
    click.connect_released(move |_, press_count, _, _| {
        match video_click::release_intent(press_count) {
            video_click::Intent::Ignore => {}
            video_click::Intent::SchedulePlayPause => {
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: video-single-click-scheduled");
                }
                if let Some(source_id) = pending_click.borrow_mut().take() {
                    source_id.remove();
                }
                let delay_ms = gtk::Settings::default()
                    .map(|settings| settings.property::<i32>("gtk-double-click-time").max(1) as u32)
                    .unwrap_or(250);
                let delayed_state = Rc::clone(&click_state);
                let delayed_pending = Rc::clone(&pending_click);
                let source_id = glib::timeout_add_local(
                    Duration::from_millis(u64::from(delay_ms)),
                    move || {
                        delayed_pending.borrow_mut().take();
                        if has_loaded_media(&delayed_state)
                            && let Some(mpv) = delayed_state.borrow().mpv.as_ref()
                            && let Err(error) = mpv.cycle_pause()
                        {
                            eprintln!("Failed to toggle playback: {error}");
                        }
                        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                            eprintln!("interaction: video-single-click-committed");
                        }
                        glib::ControlFlow::Break
                    },
                );
                pending_click.borrow_mut().replace(source_id);
            }
            video_click::Intent::CancelPlayPauseAndToggleFullscreen => {
                if let Some(source_id) = pending_click.borrow_mut().take() {
                    source_id.remove();
                }
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: video-double-click-fullscreen");
                }
                toggle_fullscreen(&click_window);
            }
        }
    });

    video_area.add_controller(click);
}

pub(crate) fn connect_player_context_menu(
    player_root: &gtk::Overlay,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) {
    let context_click = gtk::GestureClick::new();
    context_click.set_button(gdk::BUTTON_SECONDARY);
    context_click.set_propagation_phase(gtk::PropagationPhase::Bubble);

    let context_root = player_root.clone();
    let context_window = window.clone();
    let context_state = Rc::clone(&state);
    let context_toast = Rc::clone(&status_toast);
    let context_chrome = Rc::clone(&chrome);
    context_click.connect_pressed(move |gesture, _, x, y| {
        let Some(target) = context_root.pick(x, y, gtk::PickFlags::INSENSITIVE) else {
            return;
        };
        if player_context_menu_target_is_interactive(&context_root, &target) {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: player-context-menu-suppressed");
            }
            return;
        }

        gesture.set_state(gtk::EventSequenceState::Claimed);
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: player-context-menu-open x={x:.0} y={y:.0}");
        }
        show_player_context_menu(
            &context_root,
            &context_window,
            Rc::clone(&context_state),
            Rc::clone(&context_toast),
            Rc::clone(&context_chrome),
            x,
            y,
        );
    });

    player_root.add_controller(context_click);
}

pub(crate) fn player_context_menu_target_is_interactive(
    player_root: &gtk::Overlay,
    target: &gtk::Widget,
) -> bool {
    const BLOCKING_CSS_CLASSES: [&str; 5] = [
        "okp-time-label",
        "okp-timeline",
        "okp-volume-control",
        "okp-up-next-panel",
        "okp-resize-handle",
    ];

    let mut current = Some(target.clone());
    while let Some(widget) = current {
        if widget == *player_root {
            return false;
        }
        if widget.is::<gtk::Button>()
            || widget.is::<gtk::MenuButton>()
            || widget.is::<gtk::Scale>()
            || widget.is::<gtk::Scrollbar>()
            || widget.is::<gtk::Entry>()
            || widget.is::<gtk::TextView>()
            || widget.is::<gtk::SpinButton>()
            || widget.is::<gtk::DropDown>()
            || widget.is::<gtk::Switch>()
            || widget.is::<gtk::ListBoxRow>()
            || widget.is::<gtk::Popover>()
            || BLOCKING_CSS_CLASSES
                .iter()
                .any(|class| widget.has_css_class(class))
        {
            return true;
        }
        current = widget.parent();
    }
    false
}

pub(crate) fn show_player_context_menu(
    player_root: &gtk::Overlay,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
    x: f64,
    y: f64,
) {
    let popover = gtk::Popover::new();
    prepare_track_popover(&popover, PlayerPopoverKind::AdvancedCommands);
    connect_popover_chrome_pin(&popover, chrome);
    popover.set_parent(player_root);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let content = advanced_command_popover_content(&popover, parent, state, status_toast);
    set_track_popover_child(&popover, PlayerPopoverKind::AdvancedCommands, content);
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}
