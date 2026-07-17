use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

fn failure_environment() -> okp_core::playback_failure::PlaybackFailureEnvironment {
    if APP_PACKAGE_FLAVOR == "fedora-native" {
        okp_core::playback_failure::PlaybackFailureEnvironment::FedoraNative
    } else {
        okp_core::playback_failure::PlaybackFailureEnvironment::Generic
    }
}

fn diagnostic_from_reason(reason: &str) -> PlaybackFailureDiagnostic {
    okp_core::playback_failure::diagnose_mpv_failure(0, &[reason.to_owned()], failure_environment())
}

/// Record a network-source load failure: transition the transport-surface model to
/// `Failed`, remember the URL for Retry, and store the short copyable reason. The
/// the in-canvas failure card can offer Retry and Copy details.
pub(crate) fn set_load_failure(state: &Rc<RefCell<PlayerState>>, url: String, reason: String) {
    let mut state = state.borrow_mut();
    state.media_load_state = network_media::MediaLoadState::Failed;
    state.retry_load_source = Some(network_media::LoadFailureSource::url(url));
    state.last_load_diagnostic = Some(diagnostic_from_reason(&reason));
    state.last_load_error = Some(reason);
}

/// Record a local-file load failure. The path replaces any previous retry source so
/// the card can replay the failed local file without reviving a stale stream.
pub(crate) fn set_local_load_failure(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    reason: String,
) {
    let mut state = state.borrow_mut();
    state.media_load_state = network_media::MediaLoadState::Failed;
    state.retry_load_source = Some(network_media::LoadFailureSource::local(path));
    state.last_load_diagnostic = Some(diagnostic_from_reason(&reason));
    state.last_load_error = Some(reason);
}

/// Apply an `EndFile::Error` the engine fired asynchronously (a load command returned
/// `Ok`, then mpv rejected the source later — e.g. a 404 stream). The pump snapshots
/// the ended source's path/URL in the event; when it no longer matches the current
/// source (URL A failed, then the user started URL B before the next poll drained the
/// queue), the error belongs to A and must not fail B or arm the dialog with A's
/// reason. If no source is current anymore, the event is stale even when mpv omitted
/// the ended path. A `None` ended path only falls back to applying while a source is
/// active, so a missing tag never under-reports a genuine failure.
#[cfg(test)]
pub(crate) fn apply_endfile_error(
    state: &Rc<RefCell<PlayerState>>,
    error: std::ffi::c_int,
    ended_path: Option<&str>,
) {
    apply_endfile_error_with_diagnostics(state, error, ended_path, &[]);
}

pub(crate) fn apply_endfile_error_with_diagnostics(
    state: &Rc<RefCell<PlayerState>>,
    error: std::ffi::c_int,
    ended_path: Option<&str>,
    diagnostics: &[String],
) {
    eprintln!("libmpv ended the source with error {error}");
    let current_source = {
        let state = state.borrow();
        state
            .current_url
            .as_ref()
            .map(|url| network_media::LoadFailureSource::url(url.clone()))
            .or_else(|| {
                state
                    .current_file
                    .as_ref()
                    .map(|path| network_media::LoadFailureSource::local(path.clone()))
            })
    };
    let Some(current_source) = current_source else {
        eprintln!("ignoring stale EndFile::Error after the source was cleared");
        return;
    };
    let stale = ended_path.is_some_and(|ended| !current_source.matches_engine_path(ended));
    if stale {
        eprintln!(
            "ignoring stale EndFile::Error for a superseded source ({})",
            ended_path.unwrap_or_default()
        );
        return;
    }
    let mut state = state.borrow_mut();
    let diagnostic =
        okp_core::playback_failure::diagnose_mpv_failure(error, diagnostics, failure_environment());
    state.media_load_state = network_media::MediaLoadState::Failed;
    state.retry_load_source = Some(current_source);
    state.last_load_error = Some(diagnostic.details.clone());
    state.last_load_diagnostic = Some(diagnostic);
}

pub(crate) fn clear_loaded_media_state(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    if let Some(mpv) = state.mpv.as_ref() {
        mpv.set_media_source(None);
    }
    state.current_file = None;
    state.current_url = None;
    state.current_nfo_title = okp_core::nfo_metadata::NfoTitleState::NotApplicable;
    state.current_video_dimensions = None;
    advance_source_generation(&mut state);
    state.playlist.clear();
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_launch_tracks = None;
    state.next_launch_directives = None;
    state.pending_preferences = None;
    state.video_transform.reset();
    state.ab_loop = AbLoopState::default();
    // Reset the transport-surface model: nothing is loading or failed anymore.
    state.media_load_state = network_media::MediaLoadState::Idle;
    state.retry_load_source = None;
    state.last_load_error = None;
    state.last_load_diagnostic = None;
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
        load_new_source(&state, |mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => remember_loaded_url(state, url),
        Some(Err(error)) => {
            eprintln!("Failed to load URL '{url}': {error}");
            set_load_failure(state, url, format!("libmpv error {error}"));
        }
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
        load_new_source(&state, |mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => remember_loaded_media(state, path),
        Some(Err(error)) => {
            eprintln!("Failed to load media '{}': {error}", path.display());
            set_local_load_failure(state, path, format!("libmpv error {error}"));
        }
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
        load_new_source(&state, |mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_media_with_playlist(state, path, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load media '{}': {error}", path.display());
            set_local_load_failure(state, path, format!("libmpv error {error}"));
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
    let retry_source = network_media::LoadFailureSource::local(path.clone());
    let preferences_path = path.clone();
    let nfo_path = path.clone();
    let mut state = state.borrow_mut();
    advance_source_generation(&mut state);
    let directives = state.next_launch_directives.take().unwrap_or_default();
    let remembered_resume = if state.private_session || !state.settings.resume_enabled() {
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
    state.current_nfo_title = okp_core::nfo_metadata::NfoTitleState::Pending;
    state
        .nfo_title_jobs
        .resolve(state.source_generation, nfo_path);
    state.playlist.reset(playlist, &current);
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.pending_subtitles.clear();
    state.pending_resume =
        launch_args::resolve_resume(directives.resume_seconds, remembered_resume).map(|target| {
            PendingResume {
                source_generation: state.source_generation,
                target,
            }
        });
    state.pending_launch_tracks = directives.has_tracks().then_some(PendingLaunchTracks {
        source_generation: state.source_generation,
        subtitle: directives.subtitle,
        audio: directives.audio,
    });
    state.pending_preferences = preferences.map(|preferences| (preferences_path, preferences));
    // A local file is also loading until `FileLoaded` fires (near-instant on a local
    // disk, but the surface is shared with network sources for consistency).
    state.media_load_state = network_media::MediaLoadState::Loading;
    state.retry_load_source = Some(retry_source);
    state.last_load_error = None;
    state.last_load_diagnostic = None;
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
        load_new_source(&state, |mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load URL '{url}': {error}");
            set_load_failure(state, url, format!("libmpv error {error}"));
            false
        }
        None => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
    }
}

fn load_new_source(
    state: &PlayerState,
    load: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) -> Option<Result<(), okp_mpv::MpvError>> {
    let mpv = state.mpv.as_ref()?;
    Some(load_new_source_with_global_subtitle_scale(
        mpv,
        state.settings.subtitle_scale(),
        load,
    ))
}

pub(crate) fn load_new_source_with_global_subtitle_scale(
    mpv: &Mpv,
    global_scale: f64,
    load: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) -> Result<(), okp_mpv::MpvError> {
    if let Err(error) = mpv.set_subtitle_scale(global_scale) {
        eprintln!("Failed to reset subtitle size for new media: {error}");
    }
    load(mpv)
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
    advance_source_generation(&mut state);
    let directives = state.next_launch_directives.take().unwrap_or_default();
    reset_video_transform_for_new_media(&mut state);
    state.ab_loop = AbLoopState::default();
    if let Some(mpv) = state.mpv.as_ref() {
        mpv.set_media_source(None);
    }
    let current = PlaylistItem::Url(url.clone());
    state.current_file = None;
    state.current_url = Some(url);
    state.current_nfo_title = okp_core::nfo_metadata::NfoTitleState::NotApplicable;
    state.playlist.reset(playlist, &current);
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume =
        launch_args::resolve_resume(directives.resume_seconds, None).map(|target| PendingResume {
            source_generation: state.source_generation,
            target,
        });
    state.pending_launch_tracks = directives.has_tracks().then_some(PendingLaunchTracks {
        source_generation: state.source_generation,
        subtitle: directives.subtitle,
        audio: directives.audio,
    });
    state.pending_preferences = None;
    // A network source is now handed to the engine — show the loading surface until
    // the `FileLoaded` lifecycle event fires (or a failure is reported).
    state.media_load_state = network_media::MediaLoadState::Loading;
    state.retry_load_source = state
        .current_url
        .clone()
        .map(network_media::LoadFailureSource::url);
    state.last_load_error = None;
    state.last_load_diagnostic = None;
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
        let current_file = state.current_file.clone();
        let current_url = state.current_url.clone();
        if current_file.is_none() && current_url.is_none() {
            status_toast.show("Open local media first");
            return false;
        }
        let Some(count) = state.playlist.queue_insert(
            current_file.as_deref(),
            current_url.as_deref(),
            additions,
            mode,
        ) else {
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

pub(crate) fn try_pending_resume(state: &Rc<RefCell<PlayerState>>, duration: f64) {
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }

    let pending = {
        let state = state.borrow();
        state.pending_resume
    };
    let Some(pending) = pending else {
        return;
    };

    if state.borrow().source_generation != pending.source_generation {
        state.borrow_mut().pending_resume = None;
        return;
    }

    let Some(target) = pending.target.seek_position(duration) else {
        state.borrow_mut().pending_resume = None;
        return;
    };

    if seek_absolute(state, target) {
        if pending.target.origin == launch_args::ResumeOrigin::ExplicitLaunch {
            eprintln!("Applied explicit launch resume at {target:.3}s");
        }
        state.borrow_mut().pending_resume = None;
    }
}

pub(crate) fn try_pending_launch_tracks(state: &Rc<RefCell<PlayerState>>) {
    let Some(pending) = state.borrow_mut().pending_launch_tracks.take() else {
        return;
    };
    if state.borrow().source_generation != pending.source_generation {
        return;
    }

    let state = state.borrow();
    let Some(mpv) = state.mpv.as_ref() else {
        return;
    };
    if let Some(selection) = pending.audio {
        let id = track_selection_id(selection);
        if let Err(error) = mpv.select_audio(id) {
            eprintln!("Failed to apply launch audio track hint: {error}");
        }
    }
    if let Some(selection) = pending.subtitle {
        let id = track_selection_id(selection);
        if let Err(error) = mpv.select_subtitle(id) {
            eprintln!("Failed to apply launch subtitle track hint: {error}");
        }
    }
}

fn track_selection_id(selection: launch_args::TrackSelection) -> Option<i64> {
    match selection {
        launch_args::TrackSelection::Off => None,
        launch_args::TrackSelection::Id(id) => Some(i64::from(id)),
    }
}

fn advance_source_generation(state: &mut PlayerState) {
    state.source_generation = state.source_generation.wrapping_add(1);
    state.current_video_dimensions = None;
    state
        .initial_window_fit
        .begin_source(state.source_generation);
    state.seek_generation = 0;
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

    let (result, video_available) = {
        let state = state.borrow();
        let video_available = state
            .mpv
            .as_ref()
            .and_then(Mpv::observed_video_dimensions)
            .is_some();
        (
            state
                .mpv
                .as_ref()
                .map(|mpv| apply_playback_preferences(mpv, &preferences, video_available)),
            video_available,
        )
    };

    match result {
        Some(Ok(())) => {
            let mut state = state.borrow_mut();
            state.video_transform = if video_available {
                preferences.video_geometry.unwrap_or_default().normalized()
            } else {
                VideoGeometry::default()
            };
            state.pending_preferences = None;
        }
        Some(Err(error)) => eprintln!("Failed to restore playback preferences: {error}"),
        None => {}
    }
}

pub(crate) fn apply_playback_preferences(
    mpv: &Mpv,
    preferences: &history::PlaybackPreferences,
    video_available: bool,
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
    if let Some(delay) = preferences.audio_delay.and_then(finite_option) {
        mpv.set_audio_delay(delay)?;
    }
    if let Some(speed) = preferences.speed.and_then(finite_option) {
        mpv.set_speed(speed)?;
    }
    if video_available && let Some(geometry) = preferences.video_geometry {
        apply_video_geometry_to_mpv(mpv, geometry)?;
    }

    Ok(())
}

pub(crate) fn apply_video_geometry_to_mpv(
    mpv: &Mpv,
    geometry: VideoGeometry,
) -> Result<(), okp_mpv::MpvError> {
    let geometry = geometry.normalized();
    mpv.set_video_aspect_override(geometry.aspect.mpv_value())?;
    mpv.set_video_zoom(geometry.mpv_zoom())?;
    mpv.set_video_pan(geometry.pan_x, geometry.pan_y)?;
    mpv.set_video_rotation(geometry.rotation_degrees)?;
    mpv.set_video_fill_screen(geometry.fill_screen)?;
    mpv.set_video_deinterlace(geometry.deinterlace)
}

pub(crate) fn save_current_preferences(state: &Rc<RefCell<PlayerState>>) {
    save_current_preferences_impl(state, None, None, None);
}

/// Persist the subtitle delay just sent to mpv instead of re-reading the
/// asynchronous observed snapshot, which may still expose the previous value.
pub(crate) fn save_current_preferences_with_subtitle_delay(
    state: &Rc<RefCell<PlayerState>>,
    subtitle_delay: f64,
) {
    save_current_preferences_impl(state, Some(subtitle_delay), None, None);
}

/// Persist the subtitle scale just sent to mpv instead of re-reading the asynchronous observed
/// snapshot, which may still expose the previous value.
pub(crate) fn save_current_preferences_with_subtitle_scale(
    state: &Rc<RefCell<PlayerState>>,
    subtitle_scale: f64,
) {
    save_current_preferences_impl(state, None, Some(subtitle_scale), None);
}

/// Save preferences right after applying an audio delay, persisting the value
/// that was just set instead of re-reading `observed_audio_delay()`. The async
/// pump snapshot may still report the previous delay, so re-reading it here
/// could persist a stale value — a reset to `0` would otherwise re-save the old
/// `+500 ms`.
pub(crate) fn save_current_preferences_with_audio_delay(
    state: &Rc<RefCell<PlayerState>>,
    audio_delay: f64,
) {
    save_current_preferences_impl(state, None, None, Some(audio_delay));
}

fn save_current_preferences_impl(
    state: &Rc<RefCell<PlayerState>>,
    subtitle_delay_override: Option<f64>,
    subtitle_scale_override: Option<f64>,
    audio_delay_override: Option<f64>,
) {
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
        let preferences = playback_preferences_with_overrides(
            preferences,
            subtitle_delay_override,
            subtitle_scale_override,
            audio_delay_override,
        );

        (path, preferences)
    };

    let (path, preferences) = snapshot;
    let mut state = state.borrow_mut();
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save playback preferences: {error}");
    }
}

pub(crate) fn save_current_video_geometry(
    state: &Rc<RefCell<PlayerState>>,
    geometry: VideoGeometry,
) {
    let path = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        path
    };

    let mut state = state.borrow_mut();
    state.history.record_preferences(
        &path,
        history::PlaybackPreferences {
            video_geometry: Some(geometry.normalized()),
            ..history::PlaybackPreferences::default()
        },
    );
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save video geometry: {error}");
    }
}

pub(crate) fn playback_preferences_with_overrides(
    mut preferences: history::PlaybackPreferences,
    subtitle_delay: Option<f64>,
    subtitle_scale: Option<f64>,
    audio_delay: Option<f64>,
) -> history::PlaybackPreferences {
    if let Some(subtitle_delay) = subtitle_delay {
        preferences.subtitle_delay = finite_option(subtitle_delay);
    }
    if let Some(subtitle_scale) = subtitle_scale {
        preferences.subtitle_scale = finite_option(subtitle_scale);
    }
    if let Some(audio_delay) = audio_delay {
        preferences.audio_delay = finite_option(audio_delay);
    }
    preferences
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
        audio_delay: finite_option(mpv.observed_audio_delay()),
        speed: finite_option(mpv.observed_speed()),
        video_geometry: None,
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
    format!("{:.2}×", speed.clamp(0.25, 4.0))
}

pub(crate) fn speed_matches(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.005
}

pub(crate) fn save_current_progress(state: &Rc<RefCell<PlayerState>>, finished: bool) {
    let snapshot = {
        let state = state.borrow();
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(playback) = state.mpv.as_ref().map(|mpv| mpv.observed_playback_state()) else {
            return;
        };
        let preferences = (!state.private_session).then(|| {
            state
                .mpv
                .as_ref()
                .map(read_current_playback_preferences)
                .unwrap_or_default()
        });

        (
            state.private_session,
            path,
            playback,
            preferences,
            state
                .current_nfo_title
                .history_update(state.private_session),
        )
    };

    let (private_session, path, playback, preferences, title_update) = snapshot;
    let Some(duration) = playback.duration else {
        return;
    };
    let position = playback.time_pos.unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 || !position.is_finite() {
        return;
    }

    let mut state = state.borrow_mut();
    if !private_session {
        state.history.record_with_title(
            &path,
            position.clamp(0.0, duration),
            duration,
            finished,
            title_update,
        );
        state
            .history
            .record_preferences(&path, preferences.unwrap_or_default());
        if let Err(error) = state.history.save() {
            eprintln!("Failed to save history: {error}");
        }
    }
    state.progress_reporter.observe(
        private_session,
        path.to_string_lossy().as_ref(),
        position,
        duration,
        finished,
    );
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

/// The local diagnostic to surface when `text` uses the reserved `ok-player://` scheme
/// (PRD §13.4), or `None` when it is a normal path/URL the caller should open as usual.
/// Registering the scheme lets desktop integration advertise it, but external control is
/// [Later], so a request is reported rather than played — this keeps an `ok-player://`
/// token from ever reaching the media engine as undefined playback. See
/// [`ok_player_uri::interpret`].
pub(crate) fn reserved_uri_notice(text: &str) -> Option<String> {
    ok_player_uri::interpret(text).map(|request| match request {
        ok_player_uri::Request::Reserved { command } => {
            format!("ok-player:// control is reserved — \"{command}\" is not available yet")
        }
        ok_player_uri::Request::Malformed => "Ignored a malformed ok-player:// request".to_owned(),
    })
}

/// True when the YouTube resolver ([`youtube_open::YOUTUBE_RESOLVER`]) is installed on the
/// host `PATH`. mpv's `ytdl` hook shells out to it to turn a YouTube page URL into a real
/// stream; without it a YouTube link has no engine path, so the "Open URL" surface shows the
/// missing-tooling state ([`youtube_open::tooling_missing_notice`]) instead of handing the
/// URL to libmpv only to fail with a generic error. This is a quick `PATH` scan (not a
/// blocking engine read), so it is safe to call on the UI thread when the dialog is built.
pub(crate) fn youtube_resolver_available() -> bool {
    find_executable(youtube_open::YOUTUBE_RESOLVER).is_some()
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

pub(crate) fn shuffle_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}
