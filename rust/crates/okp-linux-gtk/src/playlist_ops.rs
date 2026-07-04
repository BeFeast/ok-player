use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

pub(crate) fn clear_loaded_media_state(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    if let Some(mpv) = state.mpv.as_ref() {
        mpv.set_media_source(None);
    }
    state.current_file = None;
    state.current_url = None;
    state.playlist.clear();
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    state.video_transform.reset();
    state.ab_loop = AbLoopState::default();
}

pub(crate) fn load_media_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    load_media_path_internal(state, path, true);
}

pub(crate) fn load_media_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    if !is_media_url(&url) {
        return;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => remember_loaded_url(state, url),
        Some(Err(error)) => eprintln!("Failed to load URL '{url}': {error}"),
        None => remember_loaded_url(state, url),
    }
}

pub(crate) fn load_media_path_internal(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    save_previous: bool,
) {
    if !is_media_path(&path) {
        return;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => remember_loaded_media(state, path),
        Some(Err(error)) => eprintln!("Failed to load media '{}': {error}", path.display()),
        None => remember_loaded_media(state, path),
    }
}

pub(crate) fn remember_loaded_media(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    let playlist = build_folder_playlist(&path);
    remember_loaded_media_with_playlist(state, path, playlist);
}

pub(crate) fn load_media_path_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    if !is_media_path(&path) {
        return false;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_media_with_playlist(state, path, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load media '{}': {error}", path.display());
            false
        }
        None => {
            remember_loaded_media_with_playlist(state, path, playlist);
            true
        }
    }
}

pub(crate) fn remember_loaded_media_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    playlist: Vec<PlaylistItem>,
) {
    let mut playlist = playlist
        .into_iter()
        .filter(|item| match item {
            PlaylistItem::Local(path) => is_media_path(path),
            PlaylistItem::Url(url) => is_media_url(url),
        })
        .collect::<Vec<_>>();
    if !playlist
        .iter()
        .any(|item| matches!(item, PlaylistItem::Local(item_path) if item_path == &path))
    {
        playlist.insert(0, PlaylistItem::Local(path.clone()));
    }
    let resume_path = path.clone();
    let preferences_path = path.clone();
    let mut state = state.borrow_mut();
    let resume = if state.private_session || !state.settings.resume_enabled() {
        None
    } else {
        state.history.resume_position(&path)
    };
    let preferences = if state.private_session {
        None
    } else {
        state.history.playback_preferences(&path)
    };
    reset_video_transform_for_new_media(&mut state);
    state.ab_loop = AbLoopState::default();
    if let Some(mpv) = state.mpv.as_ref() {
        mpv.set_media_source(Some(path.clone()));
    }
    let current = PlaylistItem::Local(path.clone());
    state.current_file = Some(path);
    state.current_url = None;
    state.playlist.reset(playlist, &current);
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.pending_subtitles.clear();
    state.pending_resume = resume.map(|position| (resume_path, position));
    state.pending_preferences = preferences.map(|preferences| (preferences_path, preferences));
}

pub(crate) fn remember_loaded_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    remember_loaded_url_with_playlist(state, url.clone(), vec![PlaylistItem::Url(url)]);
}

pub(crate) fn load_media_url_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    url: String,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    if !is_media_url(&url) {
        return false;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load URL '{url}': {error}");
            false
        }
        None => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
    }
}

pub(crate) fn remember_loaded_url_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    url: String,
    playlist: Vec<PlaylistItem>,
) {
    let mut playlist = playlist
        .into_iter()
        .filter(|item| match item {
            PlaylistItem::Local(path) => is_media_path(path),
            PlaylistItem::Url(url) => is_media_url(url),
        })
        .collect::<Vec<_>>();
    if !playlist
        .iter()
        .any(|item| matches!(item, PlaylistItem::Url(item_url) if item_url == &url))
    {
        playlist.insert(0, PlaylistItem::Url(url.clone()));
    }

    let mut state = state.borrow_mut();
    reset_video_transform_for_new_media(&mut state);
    state.ab_loop = AbLoopState::default();
    if let Some(mpv) = state.mpv.as_ref() {
        mpv.set_media_source(None);
    }
    let current = PlaylistItem::Url(url.clone());
    state.current_file = None;
    state.current_url = Some(url);
    state.playlist.reset(playlist, &current);
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
}

pub(crate) fn load_playlist_item_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    item: PlaylistItem,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    match item {
        PlaylistItem::Local(path) => {
            load_media_path_with_playlist(state, path, playlist, save_previous)
        }
        PlaylistItem::Url(url) => load_media_url_with_playlist(state, url, playlist, save_previous),
    }
}

pub(crate) fn load_m3u_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: &Path,
    status_toast: &StatusToast,
) -> bool {
    let playlist = match read_m3u_playlist_items(path) {
        Ok(playlist) => playlist,
        Err(M3uPlaylistReadError::NotPlaylist) => {
            status_toast.show("Choose an M3U playlist");
            return false;
        }
        Err(M3uPlaylistReadError::ReadFailed) => {
            status_toast.show("Could not read playlist");
            return false;
        }
        Err(M3uPlaylistReadError::Empty) => {
            status_toast.show("Playlist has no playable media");
            return false;
        }
    };

    let count = playlist.len();
    if let Some(first_item) = playlist.first().cloned()
        && load_playlist_item_with_playlist(state, first_item, playlist, true)
    {
        status_toast.show(&format!("Playlist opened: {count} item{}", plural_s(count)));
        return true;
    }

    status_toast.show("Could not open playlist media");
    false
}

pub(crate) fn load_m3u_playlist_silent(state: &Rc<RefCell<PlayerState>>, path: &Path) -> bool {
    let Ok(playlist) = read_m3u_playlist_items(path) else {
        return false;
    };

    let Some(first_item) = playlist.first().cloned() else {
        return false;
    };
    load_playlist_item_with_playlist(state, first_item, playlist, true)
}

pub(crate) fn read_m3u_playlist_items(
    path: &Path,
) -> Result<Vec<PlaylistItem>, M3uPlaylistReadError> {
    if !is_playlist_path(path) {
        return Err(M3uPlaylistReadError::NotPlaylist);
    }

    let text = fs::read_to_string(path).map_err(|_| M3uPlaylistReadError::ReadFailed)?;
    let entries = m3u::parse(&text, path.parent());
    let playlist = playlist_items_from_m3u_entries(&entries);
    if playlist.is_empty() {
        Err(M3uPlaylistReadError::Empty)
    } else {
        Ok(playlist)
    }
}

pub(crate) fn save_m3u_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    status_toast: &StatusToast,
) -> bool {
    let paths = {
        let state = state.borrow();
        state
            .playlist
            .items()
            .iter()
            .map(PlaylistItem::m3u_entry)
            .collect::<Vec<_>>()
    };

    if paths.is_empty() {
        status_toast.show("No playlist to save");
        return false;
    }

    let text = m3u::write(paths.iter().map(String::as_str));
    match fs::write(&path, text) {
        Ok(()) => {
            status_toast.show(&format!(
                "Playlist saved: {} item{}",
                paths.len(),
                plural_s(paths.len())
            ));
            true
        }
        Err(error) => {
            eprintln!("Failed to save playlist '{}': {error}", path.display());
            status_toast.show("Could not save playlist");
            false
        }
    }
}

pub(crate) fn queue_media_paths(
    state: &Rc<RefCell<PlayerState>>,
    paths: Vec<PathBuf>,
    mode: QueueInsertMode,
    status_toast: &StatusToast,
) -> bool {
    let additions = unique_media_paths(paths);
    if additions.is_empty() {
        status_toast.show("Choose media files");
        return false;
    }

    let count = {
        let mut state = state.borrow_mut();
        let Some(current_file) = state.current_file.clone() else {
            status_toast.show("Open local media first");
            return false;
        };
        let Some(count) = state.playlist.queue_insert(&current_file, additions, mode) else {
            status_toast.show("Already in queue");
            return false;
        };
        count
    };

    let action = match mode {
        QueueInsertMode::Append => "Queued",
        QueueInsertMode::PlayNext => "Will play next",
    };
    status_toast.show(&format!("{action}: {count} item{}", plural_s(count)));
    true
}

pub(crate) fn playlist_items_from_m3u_entries(entries: &[String]) -> Vec<PlaylistItem> {
    entries
        .iter()
        .filter_map(|entry| PlaylistItem::from_m3u_entry(entry))
        .collect()
}

pub(crate) fn playlist_save_path(mut path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_none_or(|extension| extension.is_empty())
    {
        path.set_extension("m3u");
    }
    path
}

pub(crate) fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub(crate) fn unique_media_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if is_media_path(&path) && !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

pub(crate) fn navigate_playlist(state: &Rc<RefCell<PlayerState>>, direction: isize) -> bool {
    let (index, item, playlist) = {
        let state = state.borrow();
        let Some(index) = state.playlist.peek_wrapping_index(direction) else {
            return false;
        };
        let Some(item) = state.playlist.get(index).cloned() else {
            return false;
        };
        (index, item, state.playlist.items().to_vec())
    };

    if !load_playlist_item_with_playlist(state, item, playlist, true) {
        return false; // load rejected → the cursor stays on the playing item
    }
    state.borrow_mut().playlist.set_current_index(index);
    true
}

pub(crate) fn jump_playlist_index(state: &Rc<RefCell<PlayerState>>, index: usize) -> bool {
    let (item, playlist) = {
        let state = state.borrow();
        let Some(item) = state.playlist.get(index).cloned() else {
            return false;
        };
        (item, state.playlist.items().to_vec())
    };

    if !load_playlist_item_with_playlist(state, item, playlist, true) {
        return false; // load rejected → the cursor stays on the playing item
    }
    // Committed by index after the reset inside the load, which re-finds by equality and would
    // otherwise leave the cursor on the first occurrence of a repeated entry.
    state.borrow_mut().playlist.set_current_index(index);
    true
}

pub(crate) fn advance_playlist_on_eof(state: &Rc<RefCell<PlayerState>>) -> bool {
    let (repeat_mode, target, playlist) = {
        let state = state.borrow();
        (
            state.playlist.repeat(),
            state.playlist.auto_advance_target_index(),
            state.playlist.items().to_vec(),
        )
    };

    if repeat_mode == RepeatMode::One {
        return restart_current_file(state);
    }

    let Some(index) = target else {
        return false;
    };
    let Some(next_item) = playlist.get(index).cloned() else {
        return false;
    };

    if !load_playlist_item_with_playlist(state, next_item, playlist, false) {
        return false; // load rejected → the cursor stays on the playing item
    }
    state.borrow_mut().playlist.set_current_index(index);
    true
}

/// Recovers from a file that failed to load or decode (mpv `EndFile` with an
/// error reason). Without this the shell would strand the user on a black video
/// plane behind a live OSC reading `00:00 / 00:00`, because `current_file` was
/// already committed when `loadfile` was merely queued. When the failed item has
/// a successor in the queue we advance past it so the rest of the playlist is
/// preserved; only a failure with nothing left to play unloads back to the
/// welcome surface (the tested no-media state). Either way a short message is
/// queued for the poll loop to toast so the failure is visible and actionable
/// (§2.1).
pub(crate) fn handle_playback_load_error(state: &Rc<RefCell<PlayerState>>) {
    let label = failed_media_label(&state.borrow());
    if !advance_playlist_past_failure(state) {
        clear_loaded_media_state(state);
    }
    state.borrow_mut().pending_playback_error = Some(match label {
        Some(name) => format!("Couldn't play {name}"),
        None => "Couldn't play that media".to_owned(),
    });
}

/// Loads the item that follows a failed one in play order (see
/// [`Playlist::advance_after_error_index`]), keeping the queue intact. Returns
/// `false` when there is no successor to try or the successor itself refuses to
/// load, so the caller can fall back to unloading to the welcome surface.
fn advance_playlist_past_failure(state: &Rc<RefCell<PlayerState>>) -> bool {
    let (index, item, playlist) = {
        let state = state.borrow();
        let Some(index) = state.playlist.advance_after_error_index() else {
            return false;
        };
        let Some(item) = state.playlist.get(index).cloned() else {
            return false;
        };
        (index, item, state.playlist.items().to_vec())
    };

    // Do not save progress for the file that just failed — it never played.
    if !load_playlist_item_with_playlist(state, item, playlist, false) {
        return false;
    }
    state.borrow_mut().playlist.set_current_index(index);
    true
}

/// A short, human-facing name for the media that just failed: the file's base
/// name for a local path, or the raw URL otherwise. Read before the state is
/// cleared so the toast can name what broke.
fn failed_media_label(state: &PlayerState) -> Option<String> {
    if let Some(path) = state.current_file.as_ref()
        && let Some(name) = path.file_name().and_then(|name| name.to_str())
    {
        return Some(name.to_owned());
    }
    state.current_url.clone()
}

pub(crate) fn move_playlist_item(state: &Rc<RefCell<PlayerState>>, from: usize, to: usize) -> bool {
    state.borrow_mut().playlist.reorder(from, to)
}

pub(crate) fn remove_playlist_item(state: &Rc<RefCell<PlayerState>>, index: usize) -> bool {
    state.borrow_mut().playlist.remove(index)
}

pub(crate) fn restart_current_file(state: &Rc<RefCell<PlayerState>>) -> bool {
    let path = {
        let state = state.borrow();
        let Some(path) = state.current_file.clone() else {
            return false;
        };
        let Some(mpv) = state.mpv.as_ref() else {
            return false;
        };
        if let Err(error) = mpv.load_file(&path) {
            eprintln!("Failed to repeat '{}': {error}", path.display());
            return false;
        }
        path
    };

    let preferences = state.borrow().history.playback_preferences(&path);
    let mut state = state.borrow_mut();
    state.pending_resume = None;
    state.pending_preferences = preferences.map(|preferences| (path, preferences));
    true
}

/// Drains a queued load-error message (see [`handle_playback_load_error`]) into
/// a toast. Runs from the state poll so the toast fires on the GTK main context
/// after the media has unloaded and the welcome surface is back.
pub(crate) fn try_pending_error_toast(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    let message = state.borrow_mut().pending_playback_error.take();
    if let Some(message) = message {
        status_toast.show(&message);
    }
}

pub(crate) fn try_pending_resume(state: &Rc<RefCell<PlayerState>>, duration: f64) {
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }

    let pending = {
        let state = state.borrow();
        state.pending_resume.clone()
    };
    let Some((path, target)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_resume = None;
        return;
    }

    if target > duration {
        return;
    }

    if target <= duration * 0.05 || target >= history::completion_start(duration) {
        state.borrow_mut().pending_resume = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.seek_absolute(target))
    };
    if matches!(result, Some(Ok(()))) {
        state.borrow_mut().pending_resume = None;
    } else if let Some(Err(error)) = result {
        eprintln!("Failed to resume '{}': {error}", path.display());
    }
}

pub(crate) fn try_pending_playback_preferences(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let state = state.borrow();
        state.pending_preferences.clone()
    };
    let Some((path, preferences)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_preferences = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| apply_playback_preferences(mpv, &preferences))
    };

    match result {
        Some(Ok(())) => state.borrow_mut().pending_preferences = None,
        Some(Err(error)) => eprintln!("Failed to restore playback preferences: {error}"),
        None => {}
    }
}

pub(crate) fn apply_playback_preferences(
    mpv: &Mpv,
    preferences: &history::PlaybackPreferences,
) -> Result<(), okp_mpv::MpvError> {
    let tracks = mpv.observed_tracks();

    if let Some(enabled) = preferences.audio_enabled {
        if !enabled {
            mpv.select_audio(None)?;
        } else if let Some(track_id) = preferences.audio_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio && track.id == track_id)
        {
            mpv.select_audio(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.subtitle_enabled {
        if !enabled {
            mpv.select_subtitle(None)?;
        } else if let Some(track_id) = preferences.subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_subtitle(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.secondary_subtitle_enabled {
        if !enabled {
            mpv.select_secondary_subtitle(None)?;
        } else if let Some(track_id) = preferences.secondary_subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_secondary_subtitle(Some(track_id))?;
        }
    }

    if let Some(delay) = preferences.subtitle_delay.and_then(finite_option) {
        mpv.set_subtitle_delay(delay)?;
    }
    if let Some(scale) = preferences.subtitle_scale.and_then(finite_option) {
        mpv.set_subtitle_scale(scale)?;
    }
    if let Some(speed) = preferences.speed.and_then(finite_option) {
        mpv.set_speed(speed)?;
    }

    Ok(())
}

pub(crate) fn save_current_preferences(state: &Rc<RefCell<PlayerState>>) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(preferences) = state.mpv.as_ref().map(read_current_playback_preferences) else {
            return;
        };

        (path, preferences)
    };

    let (path, preferences) = snapshot;
    let mut state = state.borrow_mut();
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save playback preferences: {error}");
    }
}

pub(crate) fn read_current_playback_preferences(mpv: &Mpv) -> history::PlaybackPreferences {
    let tracks = mpv.observed_tracks();
    let selected_audio = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Audio && track.selected);
    let selected_subtitle = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Subtitle && track.selected);
    let secondary_subtitle_id = mpv.observed_secondary_subtitle_id().filter(|id| {
        tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.id == *id)
    });
    let has_audio_tracks = tracks.iter().any(|track| track.kind == TrackKind::Audio);
    let has_subtitle_tracks = tracks.iter().any(|track| track.kind == TrackKind::Subtitle);

    history::PlaybackPreferences {
        audio_enabled: has_audio_tracks.then_some(selected_audio.is_some()),
        audio_track_id: selected_audio.map(|track| track.id),
        subtitle_enabled: has_subtitle_tracks.then_some(selected_subtitle.is_some()),
        subtitle_track_id: selected_subtitle.map(|track| track.id),
        secondary_subtitle_enabled: has_subtitle_tracks.then_some(secondary_subtitle_id.is_some()),
        secondary_subtitle_track_id: secondary_subtitle_id,
        subtitle_delay: finite_option(mpv.observed_subtitle_delay()),
        subtitle_scale: finite_option(mpv.observed_subtitle_scale()),
        speed: finite_option(mpv.observed_speed()),
    }
}

pub(crate) fn finite_option(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

pub(crate) fn read_playback_speed(state: &Rc<RefCell<PlayerState>>) -> f64 {
    state
        .borrow()
        .mpv
        .as_ref()
        .map(|mpv| mpv.observed_speed())
        .and_then(finite_option)
        .unwrap_or(1.0)
}

pub(crate) fn format_speed(speed: f64) -> String {
    format!("{:.2}x", speed.clamp(0.25, 4.0))
}

pub(crate) fn speed_matches(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.005
}

pub(crate) fn video_aspect_value(value: &str) -> &'static str {
    VIDEO_ASPECT_PRESETS
        .iter()
        .find_map(|(_, preset)| (*preset == value).then_some(*preset))
        .unwrap_or(VIDEO_ASPECT_AUTO)
}

pub(crate) fn save_current_progress(state: &Rc<RefCell<PlayerState>>, finished: bool) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(playback) = state.mpv.as_ref().map(|mpv| mpv.observed_playback_state()) else {
            return;
        };
        let preferences = state
            .mpv
            .as_ref()
            .map(read_current_playback_preferences)
            .unwrap_or_default();

        (path, playback, preferences)
    };

    let (path, playback, preferences) = snapshot;
    let Some(duration) = playback.duration else {
        return;
    };
    let position = playback.time_pos.unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 || !position.is_finite() {
        return;
    }

    let mut state = state.borrow_mut();
    state
        .history
        .record(&path, position.clamp(0.0, duration), duration, finished);
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save history: {error}");
    }
}

pub(crate) fn build_folder_playlist(path: &Path) -> Vec<PlaylistItem> {
    let Some(parent) = path.parent() else {
        return vec![PlaylistItem::Local(path.to_path_buf())];
    };

    let files = media_paths_in_directory(parent);
    if files.is_empty() {
        return vec![PlaylistItem::Local(path.to_path_buf())];
    };

    files.into_iter().map(PlaylistItem::Local).collect()
}

pub(crate) fn media_paths_in_directory(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_media_path(path))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        let left = left.file_name().and_then(|name| name.to_str());
        let right = right.file_name().and_then(|name| name.to_str());
        natural_compare::compare(left, right)
    });
    files
}

pub(crate) fn load_subtitle_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) -> bool {
    if !is_subtitle_path(&path) || !has_loaded_media(state) {
        return false;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
    };

    match result {
        Some(Ok(())) => true,
        Some(Err(error)) => {
            eprintln!(
                "Subtitle queued until media is ready '{}': {error}",
                path.display()
            );
            state.borrow_mut().pending_subtitles.push(path);
            false
        }
        None => false,
    }
}

pub(crate) fn try_pending_subtitles(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let mut state = state.borrow_mut();
        if !has_loaded_media_state(&state) || state.pending_subtitles.is_empty() {
            return;
        }

        std::mem::take(&mut state.pending_subtitles)
    };

    let mut retry = Vec::new();
    for path in pending {
        let result = {
            let state = state.borrow();
            state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
        };

        if !matches!(result, Some(Ok(()))) {
            retry.push(path);
        }
    }

    if !retry.is_empty() {
        state.borrow_mut().pending_subtitles.extend(retry);
    }
}

pub(crate) fn is_media_path(path: &Path) -> bool {
    media_formats::is_media(path)
}

pub(crate) fn is_media_url(url: &str) -> bool {
    media_formats::is_playable_url(Some(url))
}

pub(crate) fn is_subtitle_path(path: &Path) -> bool {
    media_formats::is_subtitle(path)
}

pub(crate) fn is_playlist_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "m3u" || extension == "m3u8"
        })
        .unwrap_or(false)
}

pub(crate) fn display_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

pub(crate) fn shuffle_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}
