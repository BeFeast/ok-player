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
    let volume = {
        let mut state = state.borrow_mut();
        let volume = state.volume_state.set_level(volume);
        state.pending_volume = Some(volume);
        volume
    };
    let result = state
        .borrow()
        .mpv
        .as_ref()
        .map(|mpv| mpv.set_volume(volume));
    match result {
        Some(Ok(())) => {
            save_volume_setting(state, volume);
        }
        None => {
            state.borrow_mut().pending_volume = None;
            save_volume_setting(state, volume);
        }
        Some(Err(error)) => {
            state.borrow_mut().pending_volume = None;
            eprintln!("Failed to set volume: {error}");
        }
    }
}

pub(crate) fn adjust_volume(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    delta: f64,
) {
    if state.borrow().mpv.is_none() {
        return;
    }
    let updated_volume = {
        let mut state = state.borrow_mut();
        let volume = state.volume_state.nudge(delta);
        state.pending_volume = Some(volume);
        volume
    };

    if !with_mpv(state, |mpv| mpv.set_volume(updated_volume)) {
        state.borrow_mut().pending_volume = None;
        return;
    }

    save_volume_setting(state, updated_volume);
    status_toast.show(&format!("Volume {}%", updated_volume.round() as i64));
}

pub(crate) fn toggle_volume_mute(state: &Rc<RefCell<PlayerState>>) {
    if state.borrow().mpv.is_none() {
        return;
    }
    let updated_volume = {
        let mut state = state.borrow_mut();
        let volume = state.volume_state.toggle_mute();
        state.pending_volume = Some(volume);
        volume
    };

    if !with_mpv(state, |mpv| mpv.set_volume(updated_volume)) {
        state.borrow_mut().pending_volume = None;
        return;
    }

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
    let current = state
        .borrow()
        .mpv
        .as_ref()
        .map(Mpv::observed_subtitle_scale)
        .unwrap_or(okp_core::subtitle_style::DEFAULT_SCALE);
    let target = okp_core::subtitle_style::normalized_scale(Some(current + delta));
    if with_mpv(state, |mpv| mpv.set_subtitle_scale(target)) {
        save_current_preferences_with_subtitle_scale(state, target);
    }
}

pub(crate) fn set_subtitle_style_setting(
    state: &Rc<RefCell<PlayerState>>,
    key: &str,
) -> Result<(), &'static str> {
    let style = okp_core::subtitle_style::from_key(Some(key));
    {
        let mut state = state.borrow_mut();
        state.settings.set_subtitle_style(style.key);
        if let Err(error) = state.settings.save() {
            eprintln!("Failed to save subtitle style: {error}");
            return Err("Could not save subtitle style");
        }
    }
    if !with_mpv(state, |mpv| mpv.set_subtitle_style(style.options)) && state.borrow().mpv.is_some()
    {
        eprintln!("Failed to apply subtitle style");
        return Err("Could not apply subtitle style");
    }
    Ok(())
}

pub(crate) fn screenshot_context(
    state: &Rc<RefCell<PlayerState>>,
) -> Option<(Option<PathBuf>, okp_core::screenshot::SavedCaptureContext)> {
    let (has_media, current_file, context) = {
        let state = state.borrow();
        let position = state
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_playback_state())
            .and_then(|playback| playback.time_pos);
        (
            has_loaded_media_state(&state) && state.mpv.is_some(),
            state.current_file.clone(),
            okp_core::screenshot::SavedCaptureContext {
                source_generation: state.source_generation,
                seek_generation: state.seek_generation,
                position,
            },
        )
    };

    if !has_media {
        return None;
    }

    Some((current_file, context))
}

pub(crate) fn save_screenshot(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    include_subtitles: bool,
) {
    let Some((current_file, request_context)) = screenshot_context(state) else {
        status_toast.show("Open media first");
        return;
    };
    let (directory, format) = {
        let state = state.borrow();
        (
            state
                .settings
                .screenshot_directory()
                .unwrap_or_else(screenshots::default_screenshot_dir),
            state.settings.screenshot_format(),
        )
    };
    state.borrow().screenshot_jobs.prepare_saved(
        directory,
        current_file,
        request_context,
        format,
        include_subtitles,
    );
}

pub(crate) fn copy_frame_to_clipboard(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    if screenshot_context(state).is_none() {
        status_toast.show("Open media first");
        return;
    }

    state.borrow().screenshot_jobs.prepare_clipboard();
}

pub(crate) fn drain_screenshot_jobs(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let results = state.borrow().screenshot_jobs.drain();
    for result in results {
        match result {
            screenshots::ScreenshotJobResult::SavedPrepared(Ok(target)) => {
                dispatch_screenshot_capture(
                    state,
                    status_toast,
                    screenshots::PendingCapture::Saved(target),
                );
            }
            screenshots::ScreenshotJobResult::ClipboardPrepared(Ok(path)) => {
                dispatch_screenshot_capture(
                    state,
                    status_toast,
                    screenshots::PendingCapture::Clipboard(path),
                );
            }
            screenshots::ScreenshotJobResult::SavedPublished(Ok(path)) => {
                let filename = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| "screenshot".into());
                eprintln!("Screenshot saved to {}", path.display());
                status_toast.show_screenshot(&format!("Saved {filename}"), &path);
            }
            screenshots::ScreenshotJobResult::SavedPrepared(Err(error))
            | screenshots::ScreenshotJobResult::SavedPublished(Err(error)) => {
                eprintln!("Failed to save screenshot: {error}");
                status_toast.show("Screenshot failed");
            }
            screenshots::ScreenshotJobResult::ClipboardPrepared(Err(error)) => {
                eprintln!("Failed to prepare clipboard capture: {error}");
                status_toast.show("Couldn't copy the frame");
            }
        }
    }
}

fn dispatch_screenshot_capture(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    capture: screenshots::PendingCapture,
) {
    if let screenshots::PendingCapture::Saved(target) = &capture {
        let current_context = {
            let state = state.borrow();
            if !has_loaded_media_state(&state) {
                None
            } else {
                state
                    .mpv
                    .as_ref()
                    .map(|mpv| okp_core::screenshot::SavedCaptureContext {
                        source_generation: state.source_generation,
                        seek_generation: state.seek_generation,
                        position: mpv.observed_playback_state().time_pos,
                    })
            }
        };
        let stale = match current_context {
            Some(current) => screenshots::cancel_saved_capture_if_stale(target, current),
            None => {
                screenshots::remove_temporary_capture(&target.temp_path);
                Some(okp_core::screenshot::SavedCaptureValidity::SourceChanged)
            }
        };
        if let Some(reason) = stale {
            eprintln!("Canceled stale screenshot capture: {reason:?}");
            status_toast.show("Screenshot canceled");
            return;
        }
    }

    let (path, include_subtitles) = match &capture {
        screenshots::PendingCapture::Saved(target) => (&target.temp_path, target.include_subtitles),
        screenshots::PendingCapture::Clipboard(path) => (path, false),
    };
    let request = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .ok_or_else(|| "media closed".to_owned())
            .and_then(|mpv| {
                mpv.screenshot_to_file_async(path, include_subtitles)
                    .map_err(|error| error.to_string())
            })
    };

    match request {
        Ok(request_id) => state
            .borrow_mut()
            .screenshot_jobs
            .insert_pending(request_id, capture),
        Err(error) => {
            remove_pending_capture_temp(&capture);
            eprintln!("Failed to start screenshot capture: {error}");
            status_toast.show(capture_failure_message(&capture));
        }
    }
}

pub(crate) fn complete_screenshot_capture(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    request_id: u64,
    error: i32,
) {
    let capture = state.borrow_mut().screenshot_jobs.take_pending(request_id);
    let Some(capture) = capture else {
        return;
    };

    if error < 0 {
        remove_pending_capture_temp(&capture);
        eprintln!("Screenshot command failed with code {error}");
        status_toast.show(capture_failure_message(&capture));
        return;
    }

    match capture {
        screenshots::PendingCapture::Saved(target) => {
            state.borrow().screenshot_jobs.publish_saved(target);
        }
        screenshots::PendingCapture::Clipboard(path) => {
            finish_clipboard_capture(path, status_toast)
        }
    }
}

fn finish_clipboard_capture(path: PathBuf, status_toast: &StatusToast) {
    match gdk::Texture::from_filename(&path) {
        Ok(texture) => {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_texture(&texture);
                eprintln!("Frame copied to clipboard from {}", path.display());
                status_toast.show_screenshot("Frame copied", &path);
            } else {
                status_toast.show("Clipboard unavailable");
            }
        }
        Err(error) => {
            eprintln!("Failed to load clipboard frame {}: {error}", path.display());
            status_toast.show("Couldn't copy the frame");
        }
    }
    screenshots::remove_temporary_capture(&path);
}

fn remove_pending_capture_temp(capture: &screenshots::PendingCapture) {
    match capture {
        screenshots::PendingCapture::Saved(target) => {
            screenshots::remove_temporary_capture(&target.temp_path)
        }
        screenshots::PendingCapture::Clipboard(path) => screenshots::remove_temporary_capture(path),
    }
}

fn capture_failure_message(capture: &screenshots::PendingCapture) -> &'static str {
    match capture {
        screenshots::PendingCapture::Saved(_) => "Screenshot failed",
        screenshots::PendingCapture::Clipboard(_) => "Couldn't copy the frame",
    }
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
        seek_absolute(state, time);
    }
}

/// Linux does not have a scene-detection engine wired yet. Keeping this capability explicit
/// lets the action report an honest unavailable state without starting blocking work.
pub(crate) const SCENE_DETECTION_ENGINE_AVAILABLE: bool = false;

pub(crate) const SCENE_DETECTION_UNAVAILABLE_MESSAGE: &str = "Scene detection isn't available yet";

/// Handle the explicit Detect chapters action. A future engine can transition into progress;
/// today the core model resolves immediately to Unavailable and playback continues untouched.
pub(crate) fn detect_chapters(
    detection: &Rc<Cell<chapter_math::ChapterDetection>>,
    status_toast: &StatusToast,
) {
    if detection.get().is_running() {
        return;
    }

    let next = chapter_math::ChapterDetection::begin(SCENE_DETECTION_ENGINE_AVAILABLE);
    detection.set(next);
    if matches!(next, chapter_math::ChapterDetection::Unavailable) {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: chapter-detection=unavailable");
        }
        status_toast.show(SCENE_DETECTION_UNAVAILABLE_MESSAGE);
    }
}

pub(crate) fn jump_chapter(state: &Rc<RefCell<PlayerState>>, delta: i32) {
    let target = (|| {
        let state = state.borrow();
        let mpv = state.mpv.as_ref()?;
        let chapters = mpv.observed_chapters();
        let position = mpv.observed_playback_state().time_pos?;
        let times = chapters
            .iter()
            .map(|chapter| chapter.time)
            .collect::<Vec<_>>();
        let current = chapter_math::current_index(&times, position, chapter_math::DEFAULT_EPSILON);
        chapter_math::jump_target(current, delta, chapters.len())
            .and_then(|index| chapters.get(index))
            .map(|chapter| chapter.time)
    })();

    if let Some(target) = target {
        seek_to_chapter(state, target);
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
    if !(time.is_finite() && time >= 0.0 && seek_absolute(state, time)) {
        return false;
    }
    // An absolute jump ends any run of relative seeks, so the next fine seek
    // re-bases on the observed snapshot rather than a now-unrelated projection.
    state.borrow_mut().pending_nav = None;
    true
}

/// Latest observed playback scalars for a media session, or `None` when nothing
/// is loaded. A plain snapshot read (no blocking mpv call), safe on the UI
/// thread, used to project navigation readouts.
pub(crate) fn observed_playback(state: &Rc<RefCell<PlayerState>>) -> Option<PlaybackState> {
    let state = state.borrow();
    if !has_loaded_media_state(&state) {
        return None;
    }
    state.mpv.as_ref().map(|mpv| mpv.observed_playback_state())
}

/// The transient navigation readout line (timecode + frame number when known)
/// for a projected `target` position, built from the observed frame rate.
pub(crate) fn nav_readout_for_target(state: &Rc<RefCell<PlayerState>>, target: f64) -> String {
    let fps = observed_playback(state).and_then(|playback| playback.container_fps);
    seek_readout::format_readout(target.max(0.0), fps)
}

/// Fine seek by `delta` seconds through the shared mpv seek command, then show
/// the projected timecode/frame readout in the transient toast.
pub(crate) fn seek_relative_with_readout(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    delta: f64,
) {
    let Some(playback) = observed_playback(state) else {
        return;
    };
    if !seek_relative(state, delta) {
        return;
    }

    let time = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    // Accumulate from the previous projection when the pump has not republished
    // `time_pos` yet, so two quick `→` presses report 35s then 40s, not 35s
    // twice while mpv queues both relative seeks.
    let pending = state.borrow().pending_nav;
    let next = seek_readout::advance_seek(time, delta, duration, pending);
    state.borrow_mut().pending_nav = Some(next);
    status_toast.show(&seek_readout::format_readout(
        next.projected_target,
        playback.container_fps,
    ));
}

/// Step one frame (`forward` = `.`, otherwise `,`) through the shared mpv
/// frame-step commands, which pause playback, then show the projected
/// frame readout in the transient toast.
pub(crate) fn frame_step_with_readout(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    forward: bool,
) {
    let Some(playback) = observed_playback(state) else {
        return;
    };
    let stepped = step_frame(state, forward);
    if !stepped {
        return;
    }

    let time = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    // Accumulate across repeated `.`/`,` presses so successive frame steps
    // report frame N+1, N+2, … instead of the first projected frame while the
    // pump has yet to republish `time_pos` for the paused, stepped video.
    let pending = state.borrow().pending_nav;
    let next =
        seek_readout::advance_frame_step(time, playback.container_fps, forward, duration, pending);
    state.borrow_mut().pending_nav = Some(next);
    status_toast.show(&seek_readout::format_readout(
        next.projected_target,
        playback.container_fps,
    ));
}

pub(crate) fn seek_absolute(state: &Rc<RefCell<PlayerState>>, seconds: f64) -> bool {
    let sent = with_mpv(state, |mpv| mpv.seek_absolute(seconds));
    if sent {
        advance_seek_generation(state);
    }
    sent
}

pub(crate) fn seek_relative(state: &Rc<RefCell<PlayerState>>, seconds: f64) -> bool {
    let sent = with_mpv(state, |mpv| mpv.seek_relative(seconds));
    if sent {
        advance_seek_generation(state);
    }
    sent
}

pub(crate) fn step_frame(state: &Rc<RefCell<PlayerState>>, forward: bool) -> bool {
    let sent = with_mpv(state, |mpv| {
        if forward {
            mpv.frame_step()
        } else {
            mpv.frame_back_step()
        }
    });
    if sent {
        advance_seek_generation(state);
    }
    sent
}

fn advance_seek_generation(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.seek_generation = state.seek_generation.wrapping_add(1);
}

pub(crate) fn toggle_fullscreen(window: &gtk::ApplicationWindow, state: &Rc<RefCell<PlayerState>>) {
    if restore_compact_mode(window) {
        // Compact mode is never itself fullscreen, so leaving it always resolves
        // to entering fullscreen. Record the intent now and defer the request
        // until the restored chrome has laid out.
        state.borrow_mut().fullscreen_toggle.observe(true);
        let window = window.clone();
        glib::idle_add_local_once(move || window.fullscreen());
        return;
    }
    // Decide from the eagerly-flipped intent, not the compositor's lagging
    // `is_fullscreen`, so a rapid second toggle still alternates instead of
    // repeating the previous request. See [`fullscreen_toggle`].
    match state.borrow_mut().fullscreen_toggle.toggle() {
        fullscreen_toggle::FullscreenAction::Enter => window.fullscreen(),
        fullscreen_toggle::FullscreenAction::Leave => window.unfullscreen(),
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

pub(crate) fn apply_video_geometry_action(
    state: &Rc<RefCell<PlayerState>>,
    action: VideoGeometryAction,
    status_toast: &StatusToast,
) {
    let target = {
        let state = state.borrow();
        let video_available = has_loaded_media_state(&state)
            && state
                .mpv
                .as_ref()
                .and_then(Mpv::observed_video_dimensions)
                .is_some();
        if !state
            .video_transform
            .action_enabled(video_available, action)
        {
            return;
        }
        let mut target = state.video_transform;
        target.apply(action);
        target
    };

    if with_mpv(state, |mpv| apply_video_geometry_to_mpv(mpv, target)) {
        state.borrow_mut().video_transform = target;
        save_current_video_geometry(state, target);
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!(
                "interaction: video-geometry={action:?} zoom={} pan=({:.1},{:.1}) rotation={} fill={} deinterlace={}",
                target.zoom_percent(),
                target.pan_x,
                target.pan_y,
                target.rotation_degrees,
                target.fill_screen,
                target.deinterlace
            );
        }
        status_toast.show(&video_geometry_message(action, target));
    } else {
        status_toast.show("Could not update video");
    }
}

pub(crate) fn video_geometry_message(
    action: VideoGeometryAction,
    geometry: VideoGeometry,
) -> String {
    match action {
        VideoGeometryAction::SetAspect(aspect) => format!("Aspect: {}", aspect.label()),
        VideoGeometryAction::ZoomIn | VideoGeometryAction::ZoomOut => {
            format!("Zoom: {}%", geometry.zoom_percent())
        }
        VideoGeometryAction::PanLeft => "Pan: left".to_owned(),
        VideoGeometryAction::PanRight => "Pan: right".to_owned(),
        VideoGeometryAction::PanUp => "Pan: up".to_owned(),
        VideoGeometryAction::PanDown => "Pan: down".to_owned(),
        VideoGeometryAction::Center => "Image centered".to_owned(),
        VideoGeometryAction::RotateClockwise => {
            format!("Rotation: {}°", geometry.rotation_degrees)
        }
        VideoGeometryAction::ToggleFillScreen => format!(
            "Fill screen {}",
            if geometry.fill_screen { "on" } else { "off" }
        ),
        VideoGeometryAction::ToggleDeinterlace => format!(
            "Deinterlace {}",
            if geometry.deinterlace { "on" } else { "off" }
        ),
        VideoGeometryAction::Reset => "Video reset".to_owned(),
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
