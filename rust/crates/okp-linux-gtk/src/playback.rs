use super::*;

pub(crate) fn repeat_mode_label(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "Repeat Off",
        RepeatMode::One => "Repeat One",
        RepeatMode::All => "Repeat All",
    }
}

pub(crate) fn apply_playback_settings_defaults(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    let repeat_mode = RepeatMode::from_settings_value(state.settings.repeat_mode());
    let auto_advance = state.settings.auto_advance_enabled();
    let shuffle = state.settings.shuffle_enabled();
    state.playlist.reseed(shuffle_seed());
    state.playlist.set_repeat(repeat_mode);
    state.playlist.set_auto_advance(auto_advance);
    state.playlist.set_shuffle(shuffle);
}

pub(crate) fn with_mpv(
    state: &Rc<RefCell<PlayerState>>,
    command: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) -> bool {
    if let Some(mpv) = state.borrow().mpv.as_ref()
        && let Err(error) = command(mpv)
    {
        eprintln!("mpv command failed: {error}");
        return false;
    }

    state.borrow().mpv.is_some()
}

pub(crate) fn has_loaded_media(state: &Rc<RefCell<PlayerState>>) -> bool {
    has_loaded_media_state(&state.borrow())
}

pub(crate) fn has_loaded_media_state(state: &PlayerState) -> bool {
    state.current_file.is_some() || state.current_url.is_some()
}

pub(crate) fn set_volume_from_ui(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let result = state
        .borrow()
        .mpv
        .as_ref()
        .map(|mpv| mpv.set_volume(volume));
    match result {
        Some(Ok(())) | None => save_volume_setting(state, volume),
        Some(Err(error)) => eprintln!("Failed to set volume: {error}"),
    }
}

pub(crate) fn adjust_volume(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    let updated_volume = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        let volume = mpv.observed_playback_state().volume.unwrap_or(100.0);
        let updated_volume = (volume + delta).clamp(0.0, 130.0);
        if let Err(error) = mpv.set_volume(updated_volume) {
            eprintln!("Failed to set volume: {error}");
            return;
        }
        updated_volume
    };

    save_volume_setting(state, updated_volume);
}

pub(crate) fn save_volume_setting(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let mut state = state.borrow_mut();
    state.settings.set_volume(volume);
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save settings: {error}");
    }
}

pub(crate) fn save_audio_device_setting(
    state: &Rc<RefCell<PlayerState>>,
    device: &str,
    status_toast: Option<&StatusToast>,
) {
    let mut state = state.borrow_mut();
    state.settings.set_audio_device(device);
    state.pending_audio_device_restore = None;
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save audio device setting: {error}");
        if let Some(status_toast) = status_toast {
            status_toast.show("Could not save audio output");
        }
    }
}

pub(crate) fn adjust_subtitle_delay(state: &Rc<RefCell<PlayerState>>, delta_seconds: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_delay(delta_seconds)) {
        save_current_preferences(state);
    }
}

pub(crate) fn adjust_subtitle_scale(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_scale(delta)) {
        save_current_preferences(state);
    }
}

pub(crate) fn screenshot_context(
    state: &Rc<RefCell<PlayerState>>,
) -> Option<(Option<PathBuf>, Option<f64>)> {
    let (has_mpv, current_file, position) = {
        let state = state.borrow();
        let position = state
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_playback_state())
            .and_then(|playback| playback.time_pos);
        (state.mpv.is_some(), state.current_file.clone(), position)
    };

    if !has_mpv {
        return None;
    }

    Some((current_file, position))
}

pub(crate) fn save_screenshot(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    include_subtitles: bool,
) {
    let Some((current_file, position)) = screenshot_context(state) else {
        return;
    };
    let (dir, format) = {
        let state = state.borrow();
        (
            screenshots::screenshot_dir(state.settings.screenshot_directory()),
            state.settings.screenshot_format(),
        )
    };
    let Some(path) =
        screenshots::next_screenshot_path(&dir, current_file.as_deref(), position, format)
    else {
        // The directory was unwritable or every candidate name was taken; refuse to overwrite
        // and surface the failure instead of blocking playback.
        eprintln!("Could not prepare a screenshot path in {}", dir.display());
        status_toast.show("Couldn't save the screenshot");
        return;
    };

    // `screenshot-to-file` captures the decoded frame (mode video/subtitles), never the
    // on-screen window, so the confirmation toast below can never appear in the saved image.
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.screenshot_to_file(&path, include_subtitles)
    };

    match result {
        Ok(()) => {
            let filename = path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| "screenshot.png".into());
            eprintln!("Screenshot saved to {}", path.display());
            status_toast.show(&format!("Screenshot saved: {filename}"));
        }
        Err(error) => {
            eprintln!("Failed to save screenshot to {}: {error}", path.display());
            status_toast.show("Screenshot failed");
        }
    }
}

pub(crate) fn copy_frame_to_clipboard(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    if screenshot_context(state).is_none() {
        return;
    }

    let Some(path) = screenshots::next_clipboard_frame_path() else {
        eprintln!("Could not prepare a temp path for the clipboard frame");
        status_toast.show("Couldn't copy the frame");
        return;
    };
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.screenshot_to_file(&path, false)
    };

    if let Err(error) = result {
        eprintln!(
            "Failed to capture frame for clipboard at {}: {error}",
            path.display()
        );
        status_toast.show("Couldn't copy the frame");
        let _ = fs::remove_file(&path);
        return;
    }

    match gdk::Texture::from_filename(&path) {
        Ok(texture) => {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_texture(&texture);
                eprintln!("Frame copied to clipboard from {}", path.display());
                status_toast.show("Frame copied");
            } else {
                status_toast.show("Clipboard unavailable");
            }
        }
        Err(error) => {
            eprintln!("Failed to load clipboard frame {}: {error}", path.display());
            status_toast.show("Couldn't copy the frame");
        }
    }
    let _ = fs::remove_file(&path);
}

pub(crate) fn copy_current_time(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let time = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_playback_state())
            .and_then(|playback| playback.time_pos)
            .filter(|time| time.is_finite() && *time >= 0.0)
    };

    let Some(time) = time else {
        status_toast.show("Open media first");
        return;
    };

    let text = time_code::format(time);
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(&text);
        status_toast.show(&format!("Copied {text}"));
    } else {
        status_toast.show("Clipboard unavailable");
    }
}

pub(crate) fn open_current_file_location(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    let path = state.borrow().current_file.clone();
    let Some(path) = path else {
        status_toast.show("Not a local file");
        return;
    };

    if show_file_in_file_manager(&path) {
        status_toast.show("Opened file location");
    } else {
        status_toast.show("Could not open the folder");
    }
}

pub(crate) fn show_file_in_file_manager(path: &Path) -> bool {
    if try_file_manager_show_items(path) {
        return true;
    }

    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return false;
    };

    Command::new("xdg-open").arg(parent).spawn().is_ok()
}

pub(crate) fn try_file_manager_show_items(path: &Path) -> bool {
    let uri = gtk::gio::File::for_path(path).uri().to_string();
    Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .spawn()
        .is_ok()
}

pub(crate) fn seek_to_chapter(state: &Rc<RefCell<PlayerState>>, time: f64) {
    if time.is_finite() && time >= 0.0 {
        with_mpv(state, |mpv| mpv.seek_absolute(time));
    }
}

/// Drop a bookmark at the current playhead. Bookmarks are per-file position marks
/// persisted in the shared history schema, so this needs a local file (streams are not
/// tracked) and honours the private session — matching Windows `HistoryService`, whose
/// writers no-op when incognito. The outcome is toasted after the borrow is released.
pub(crate) fn add_bookmark_at_position(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    let result: Result<f64, &'static str> = {
        let mut state = state.borrow_mut();
        if state.private_session {
            Err("Private session — bookmark not saved")
        } else if let Some(path) = state.current_file.clone() {
            let position = state
                .mpv
                .as_ref()
                .map(|mpv| mpv.observed_playback_state())
                .and_then(|playback| playback.time_pos)
                .filter(|time| time.is_finite() && *time >= 0.0);
            match position {
                None => Err("Open media first"),
                Some(position) => match state.history.add_bookmark_persisted(&path, position) {
                    Ok(true) => Ok(position),
                    Ok(false) => Err("Bookmark already here"),
                    Err(error) => {
                        eprintln!("Failed to save bookmark: {error}");
                        Err("Couldn't save bookmark")
                    }
                },
            }
        } else {
            Err("Bookmarks need a local file")
        }
    };

    match result {
        Ok(position) => {
            status_toast.show(&format!("Bookmarked {}", time_code::format_clock(position)))
        }
        Err(message) => status_toast.show(message),
    }
}

/// Remove the bookmark at `time` for the current file (used by a bookmark row's own
/// trash button). Removal is a deliberate edit, so it is allowed in a private session —
/// only *creating* new marks is suppressed there.
pub(crate) fn remove_bookmark_at(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    time: f64,
) {
    let message = {
        let mut state = state.borrow_mut();
        let Some(path) = state.current_file.clone() else {
            return;
        };
        match state.history.remove_bookmark_persisted(&path, time) {
            Ok(true) => Some("Bookmark removed"),
            // Nothing matched — the row is already gone, so stay quiet.
            Ok(false) => None,
            Err(error) => {
                eprintln!("Failed to save history after removing bookmark: {error}");
                Some("Couldn't remove bookmark")
            }
        }
    };

    if let Some(message) = message {
        status_toast.show(message);
    }
}

pub(crate) fn seek_to_time(state: &Rc<RefCell<PlayerState>>, time: f64) -> bool {
    time.is_finite() && time >= 0.0 && with_mpv(state, |mpv| mpv.seek_absolute(time))
}

pub(crate) fn toggle_fullscreen(window: &gtk::ApplicationWindow) {
    if window.is_fullscreen() {
        window.unfullscreen();
    } else {
        window.fullscreen();
    }
}

pub(crate) fn toggle_ab_loop(state: &Rc<RefCell<PlayerState>>, status_toast: &Rc<StatusToast>) {
    let was_active = state.borrow().ab_loop.is_active();
    if state.borrow().mpv.is_none() {
        status_toast.show("Open media first");
        return;
    }
    if !with_mpv(state, |mpv| mpv.toggle_ab_loop()) {
        status_toast.show("Could not update A-B loop");
        return;
    }

    // The `ab-loop-a`/`ab-loop-b` changes are delivered to the event pump
    // asynchronously, so the settled endpoints — and the toast describing them —
    // are read from the observed snapshot a beat later instead of a blocking
    // read on the UI thread.
    let state = Rc::clone(state);
    let status_toast = Rc::clone(status_toast);
    glib::timeout_add_local_once(AB_LOOP_SETTLE_DELAY, move || {
        let ab_loop = state
            .borrow()
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_ab_loop_state())
            .unwrap_or_default();
        state.borrow_mut().ab_loop = ab_loop;
        if let Some(message) = ab_loop_message(ab_loop, was_active) {
            status_toast.show(&message);
        }
    });
}

pub(crate) fn sync_ab_loop_state(state: &Rc<RefCell<PlayerState>>, has_media: bool) {
    let ab_loop = if has_media {
        state
            .borrow()
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_ab_loop_state())
            .unwrap_or_default()
    } else {
        AbLoopState::default()
    };
    state.borrow_mut().ab_loop = ab_loop;
}

pub(crate) fn ab_loop_message(ab_loop: AbLoopState, was_active: bool) -> Option<String> {
    match (ab_loop.a, ab_loop.b) {
        (Some(a), Some(b)) => Some(format!(
            "A-B loop: {} - {}",
            time_code::format_clock(a),
            time_code::format_clock(b)
        )),
        (Some(a), None) => Some(format!("A-B loop: start at {}", time_code::format_clock(a))),
        (None, Some(b)) => Some(format!("A-B loop: end at {}", time_code::format_clock(b))),
        (None, None) if was_active => Some("A-B loop cleared".to_owned()),
        _ => None,
    }
}

pub(crate) fn set_video_aspect(
    state: &Rc<RefCell<PlayerState>>,
    aspect: &str,
    status_toast: &StatusToast,
) {
    let aspect = video_aspect_value(aspect);
    if with_mpv(state, |mpv| mpv.set_video_aspect_override(aspect)) {
        state.borrow_mut().video_transform.set_aspect(aspect);
        if aspect == VIDEO_ASPECT_AUTO {
            status_toast.show("Aspect: Auto");
        } else {
            status_toast.show(&format!("Aspect: {aspect}"));
        }
    } else {
        status_toast.show("Could not update video");
    }
}

pub(crate) fn rotate_video_clockwise(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let rotation = {
        let state = state.borrow();
        (state.video_transform.rotation + 90).rem_euclid(360)
    };
    if with_mpv(state, |mpv| mpv.set_video_rotation(rotation)) {
        state.borrow_mut().video_transform.rotate_clockwise();
        status_toast.show("Rotated 90°");
    } else {
        status_toast.show("Could not rotate video");
    }
}

pub(crate) fn toggle_video_fill_screen(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    let enabled = {
        let state = state.borrow();
        !state.video_transform.fill_screen
    };
    if with_mpv(state, |mpv| mpv.set_video_fill_screen(enabled)) {
        state.borrow_mut().video_transform.toggle_fill_screen();
        status_toast.show(if enabled {
            "Fill screen on"
        } else {
            "Fill screen off"
        });
    } else {
        status_toast.show("Could not update video");
    }
}

pub(crate) fn reset_video_transform(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    if with_mpv(state, |mpv| mpv.reset_video_transform()) {
        state.borrow_mut().video_transform.reset();
        status_toast.show("Video reset");
    } else {
        status_toast.show("Could not reset video");
    }
}

pub(crate) fn reset_video_transform_for_new_media(state: &mut PlayerState) {
    state.video_transform.reset();
    if let Some(mpv) = state.mpv.as_ref()
        && let Err(error) = mpv.reset_video_transform()
    {
        eprintln!("Failed to reset video transform: {error}");
    }
}

pub(crate) fn cycle_repeat_mode(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let repeat_mode = state.borrow().playlist.repeat().cycle();
    set_repeat_mode_from_ui(state, status_toast, repeat_mode);
}

pub(crate) fn set_repeat_mode_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    repeat_mode: RepeatMode,
) {
    let mut state = state.borrow_mut();
    state.playlist.set_repeat(repeat_mode);
    state.settings.set_repeat_mode(repeat_mode.settings_value());
    save_settings_or_toast(&mut state, status_toast);
}

pub(crate) fn toggle_shuffle(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = !state.borrow().playlist.shuffle();
    set_shuffle_from_ui(state, status_toast, enabled);
}

pub(crate) fn set_shuffle_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    enabled: bool,
) {
    let mut state = state.borrow_mut();
    state.playlist.set_shuffle(enabled);
    state.settings.set_shuffle_enabled(enabled);
    save_settings_or_toast(&mut state, status_toast);
}

pub(crate) fn set_playback_speed_from_ui(state: &Rc<RefCell<PlayerState>>, speed: f64) {
    if with_mpv(state, |mpv| mpv.set_speed(speed)) {
        save_current_preferences(state);
    }
}

pub(crate) fn toggle_auto_advance(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let mut state = state.borrow_mut();
    let enabled = !state.playlist.auto_advance();
    state.playlist.set_auto_advance(enabled);
    state.settings.set_auto_advance_enabled(enabled);
    save_settings_or_toast(&mut state, status_toast);
}

pub(crate) fn toggle_private_session(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = {
        let mut state = state.borrow_mut();
        state.private_session = !state.private_session;
        if state.private_session {
            state.pending_resume = None;
            state.pending_preferences = None;
        }
        state.private_session
    };

    status_toast.show(if enabled {
        "Private session on"
    } else {
        "Private session off"
    });
}

pub(crate) fn clear_history(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let mut state = state.borrow_mut();
    state.history.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    match state.history.save() {
        Ok(()) => status_toast.show("History cleared"),
        Err(error) => {
            eprintln!("Failed to clear history: {error}");
            status_toast.show("Could not clear history");
        }
    }
}

pub(crate) fn close_current_media(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) -> bool {
    if !has_loaded_media(state) {
        return false;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::stop)
    };

    match result {
        Some(Ok(())) | None => {
            clear_loaded_media_state(state);
            status_toast.show("Media closed");
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to close media: {error}");
            status_toast.show("Could not close media");
            false
        }
    }
}
