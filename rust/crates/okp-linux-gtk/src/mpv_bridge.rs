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

        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);

        apply_launch_args(&realize_state, &launch_args);
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |_, width, height| {
        resize_state.borrow_mut().render_target_size =
            (width > 0 && height > 0).then_some(okp_mpv::RenderTargetSize { width, height });
    });

    let render_state = Rc::clone(&state);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let viewport_size = current_render_target_size();
        let widget_width = area.width();
        let widget_height = area.height();
        let scale_factor = area.scale_factor();
        let mut state = render_state.borrow_mut();
        let target_size = resolve_render_target_size(
            viewport_size,
            state.render_target_size,
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

    let loaded = load_launch_args(state, launch_args);
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
    let StatePollContext {
        updating_seek,
        updating_volume,
        chrome,
        empty_surface,
        mpris_snapshot,
        mpris_signals,
    } = context;
    glib::timeout_add_local(Duration::from_millis(200), move || {
        drain_mpv_events(&state);
        try_pending_audio_device_restore(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok());
        let has_media = has_loaded_media(&state);
        let has_playlist = state.borrow().playlist.len() > 1;
        {
            let state = state.borrow();
            update_mpris_snapshot(&mpris_snapshot, &mpris_signals, &state, playback);
        }
        sync_ab_loop_state(&state, has_media);
        empty_surface.set_has_media(has_media);
        drain_thumbnail_events(&controls);
        update_up_next_panel(&controls, &state, &chrome);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);
            chrome.set_auto_hide_enabled(has_media && !playback.paused);

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
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
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

            if let Some(volume) = playback.volume {
                updating_volume.set(true);
                controls.volume.set_value(volume.clamp(0.0, 130.0));
                updating_volume.set(false);
            }

            controls
                .elapsed_label
                .set_text(&time_code::format_clock(time_pos));
            controls
                .duration_label
                .set_text(&time_code::format_clock(duration));
        } else {
            chrome.set_auto_hide_enabled(false);
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls
                .play_button
                .set_icon_name("media-playback-start-symbolic");
            controls.play_button.set_tooltip_text(Some("Play (Space)"));
            controls.speed_button.set_label("1.00x");
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(false);
            updating_seek.set(true);
            controls.seek.set_range(0.0, 1.0);
            controls.seek.set_value(0.0);
            updating_seek.set(false);
            controls.elapsed_label.set_text("00:00");
            controls.duration_label.set_text("00:00");
        }

        glib::ControlFlow::Continue
    });
}

pub(crate) fn connect_video_clicks(
    video_area: &gtk::GLArea,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(1);

    let click_window = window.clone();
    click.connect_released(move |_, press_count, _, _| {
        if press_count == 2 {
            toggle_fullscreen(&click_window);
        }
    });

    video_area.add_controller(click);

    let context_click = gtk::GestureClick::new();
    context_click.set_button(3);

    let context_area = video_area.clone();
    let context_window = window.clone();
    let context_state = Rc::clone(&state);
    let context_toast = Rc::clone(&status_toast);
    context_click.connect_pressed(move |_, _, x, y| {
        show_video_context_menu(
            &context_area,
            &context_window,
            Rc::clone(&context_state),
            Rc::clone(&context_toast),
            x,
            y,
        );
    });

    video_area.add_controller(context_click);
}

pub(crate) fn show_video_context_menu(
    video_area: &gtk::GLArea,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    x: f64,
    y: f64,
) {
    let popover = gtk::Popover::new();
    prepare_track_popover(&popover);
    popover.set_parent(video_area);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let content = command_popover_content(&popover, parent, state, status_toast);
    set_track_popover_child(&popover, content);
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}
