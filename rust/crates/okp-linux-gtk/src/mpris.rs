use super::*;

pub(crate) fn create_mpris_controller() -> (
    MprisController,
    mpsc::Receiver<MprisCommand>,
    mpsc::Receiver<MprisSignal>,
) {
    let (commands, receiver) = mpsc::channel();
    let (signals, signal_receiver) = mpsc::channel();
    (
        MprisController {
            snapshot: Arc::new(Mutex::new(MprisSnapshot::default())),
            commands,
            signals,
        },
        receiver,
        signal_receiver,
    )
}

pub(crate) fn start_mpris_service(
    controller: MprisController,
    signal_receiver: mpsc::Receiver<MprisSignal>,
) {
    if env::var_os("OKP_DISABLE_MPRIS").is_some() {
        return;
    }

    let spawn_result = thread::Builder::new()
        .name("okp-mpris".to_owned())
        .spawn(move || {
            if let Err(error) = run_mpris_service(controller, signal_receiver) {
                eprintln!("MPRIS service unavailable: {error}");
            }
        });

    if let Err(error) = spawn_result {
        eprintln!("Failed to start MPRIS thread: {error}");
    }
}

pub(crate) fn run_mpris_service(
    controller: MprisController,
    signal_receiver: mpsc::Receiver<MprisSignal>,
) -> zbus::Result<()> {
    let root = MprisRoot {
        commands: controller.commands.clone(),
    };
    let player = MprisPlayer {
        snapshot: Arc::clone(&controller.snapshot),
        commands: controller.commands.clone(),
    };
    let track_list = MprisTrackList {
        snapshot: controller.snapshot,
        commands: controller.commands,
    };
    let connection = zbus::blocking::connection::Builder::session()?
        .serve_at(MPRIS_OBJECT_PATH, root)?
        .serve_at(MPRIS_OBJECT_PATH, player)?
        .serve_at(MPRIS_OBJECT_PATH, track_list)?
        .name(MPRIS_BUS_NAME)?
        .build()?;

    while let Ok(signal) = signal_receiver.recv() {
        emit_mpris_signal(&connection, signal)?;
    }

    Ok(())
}

pub(crate) fn emit_mpris_signal(
    connection: &zbus::blocking::Connection,
    signal: MprisSignal,
) -> zbus::Result<()> {
    match signal {
        MprisSignal::PlayerPropertiesInvalidated(properties) if !properties.is_empty() => {
            let changed: HashMap<&str, Value<'_>> = HashMap::new();
            connection.emit_signal(
                None::<&str>,
                MPRIS_OBJECT_PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.mpris.MediaPlayer2.Player",
                    changed,
                    properties.as_slice(),
                ),
            )
        }
        MprisSignal::TrackListPropertiesInvalidated(properties) if !properties.is_empty() => {
            let changed: HashMap<&str, Value<'_>> = HashMap::new();
            connection.emit_signal(
                None::<&str>,
                MPRIS_OBJECT_PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.mpris.MediaPlayer2.TrackList",
                    changed,
                    properties.as_slice(),
                ),
            )
        }
        MprisSignal::TrackListReplaced {
            tracks,
            current_track,
        } => connection.emit_signal(
            None::<&str>,
            MPRIS_OBJECT_PATH,
            "org.mpris.MediaPlayer2.TrackList",
            "TrackListReplaced",
            &(tracks, current_track),
        ),
        MprisSignal::Seeked(position_us) => connection.emit_signal(
            None::<&str>,
            MPRIS_OBJECT_PATH,
            "org.mpris.MediaPlayer2.Player",
            "Seeked",
            &(position_us,),
        ),
        _ => Ok(()),
    }
}

pub(crate) fn connect_mpris_commands(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    commands: mpsc::Receiver<MprisCommand>,
) {
    let window = window.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(command) = commands.try_recv() {
            handle_mpris_command(&window, &state, &status_toast, command);
        }
        glib::ControlFlow::Continue
    });
}

pub(crate) fn handle_mpris_command(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    command: MprisCommand,
) {
    match command {
        MprisCommand::Raise => window.present(),
        MprisCommand::Quit => window.close(),
        MprisCommand::Play => set_playback_paused(state, false),
        MprisCommand::Pause => set_playback_paused(state, true),
        MprisCommand::PlayPause => {
            with_mpv(state, |mpv| mpv.cycle_pause());
        }
        MprisCommand::Stop => {
            close_current_media(state, status_toast);
        }
        MprisCommand::Previous => {
            navigate_playlist(state, -1);
        }
        MprisCommand::Next => {
            navigate_playlist(state, 1);
        }
        MprisCommand::SeekBy(offset_us) => {
            let seconds = offset_us as f64 / 1_000_000.0;
            with_mpv(state, |mpv| mpv.seek_relative(seconds));
        }
        MprisCommand::SetPosition(position_us) => {
            let seconds = position_us.max(0) as f64 / 1_000_000.0;
            with_mpv(state, |mpv| mpv.seek_absolute(seconds));
        }
        MprisCommand::SetVolume(volume) => {
            if let Some(volume) = mpris_volume_to_mpv_percent(volume) {
                set_volume_from_ui(state, volume);
            }
        }
        MprisCommand::SetRate(rate) => {
            if let Some(speed) = mpris_rate_to_mpv_speed(rate) {
                set_playback_speed_from_ui(state, speed);
            }
        }
        MprisCommand::SetLoopStatus(status) => {
            if let Some(repeat_mode) = mpris_repeat_mode(&status) {
                set_repeat_mode_from_ui(state, status_toast, repeat_mode);
            }
        }
        MprisCommand::SetShuffle(shuffle) => {
            set_shuffle_from_ui(state, status_toast, shuffle);
        }
        MprisCommand::GoToTrack(track_id) => {
            let target = {
                let state = state.borrow();
                mpris_tracklist_target_for_id(&state, &track_id)
            };
            if let Some((index, item)) = target {
                if state.borrow().playlist.is_empty() {
                    match item {
                        PlaylistItem::Local(path) => load_media_path(state, path),
                        PlaylistItem::Url(url) => load_media_url(state, url),
                    }
                } else {
                    jump_playlist_index(state, index);
                }
            }
        }
        MprisCommand::OpenUri(uri) => {
            if let Some(path) = file_uri_path(&uri) {
                load_media_path(state, path);
            } else if is_media_url(&uri) {
                load_media_url(state, uri);
            }
        }
    }
}

pub(crate) fn set_playback_paused(state: &Rc<RefCell<PlayerState>>, paused: bool) {
    let should_toggle = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        match mpv.playback_state() {
            Ok(playback) => playback.paused != paused,
            Err(error) => {
                eprintln!("Failed to read playback state for MPRIS command: {error}");
                false
            }
        }
    };

    if should_toggle {
        with_mpv(state, |mpv| mpv.cycle_pause());
    }
}

pub(crate) fn update_mpris_snapshot(
    snapshot: &Arc<Mutex<MprisSnapshot>>,
    signals: &mpsc::Sender<MprisSignal>,
    state: &PlayerState,
    playback: Option<PlaybackState>,
) {
    let next = mpris_snapshot_from_state(state, playback);
    let (invalidated, tracklist_invalidated, tracklist_replaced, seeked_position) =
        if let Ok(mut snapshot) = snapshot.lock() {
            let invalidated = mpris_invalidated_properties(&snapshot, &next);
            let tracklist_invalidated = mpris_tracklist_invalidated_properties(&snapshot, &next);
            let tracklist_replaced = mpris_tracklist_replaced_signal(&snapshot, &next);
            let seeked_position = mpris_seeked_position(&snapshot, &next);
            *snapshot = next;
            (
                invalidated,
                tracklist_invalidated,
                tracklist_replaced,
                seeked_position,
            )
        } else {
            (Vec::new(), Vec::new(), None, None)
        };

    if !invalidated.is_empty() {
        let _ = signals.send(MprisSignal::PlayerPropertiesInvalidated(invalidated));
    }

    if !tracklist_invalidated.is_empty() {
        let _ = signals.send(MprisSignal::TrackListPropertiesInvalidated(
            tracklist_invalidated,
        ));
    }

    if let Some((tracks, current_track)) = tracklist_replaced {
        let _ = signals.send(MprisSignal::TrackListReplaced {
            tracks,
            current_track,
        });
    }

    if let Some(position_us) = seeked_position {
        let _ = signals.send(MprisSignal::Seeked(position_us));
    }
}

pub(crate) fn mpris_tracklist_invalidated_properties(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Vec<&'static str> {
    (previous.tracklist_track_ids() != next.tracklist_track_ids())
        .then_some(vec!["Tracks"])
        .unwrap_or_default()
}

pub(crate) fn mpris_tracklist_replaced_signal(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Option<(Vec<OwnedObjectPath>, OwnedObjectPath)> {
    if previous.tracklist == next.tracklist && previous.current_track_id == next.current_track_id {
        return None;
    }

    Some((
        next.tracklist_track_ids(),
        next.current_track_id
            .clone()
            .unwrap_or_else(mpris_no_track_id),
    ))
}

pub(crate) fn mpris_invalidated_properties(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Vec<&'static str> {
    let mut properties = Vec::new();

    if previous.playback_status() != next.playback_status() {
        properties.push("PlaybackStatus");
    }

    if previous.has_media != next.has_media
        || previous.track_id != next.track_id
        || previous.title != next.title
        || previous.uri != next.uri
        || previous.art_url != next.art_url
        || previous.duration_us != next.duration_us
    {
        properties.push("Metadata");
    }

    if previous.has_media != next.has_media {
        properties.push("CanPlay");
        properties.push("CanPause");
    }

    if previous.duration_us != next.duration_us {
        properties.push("CanSeek");
    }

    if previous.can_go_next != next.can_go_next {
        properties.push("CanGoNext");
    }

    if previous.can_go_previous != next.can_go_previous {
        properties.push("CanGoPrevious");
    }

    if (previous.volume - next.volume).abs() > f64::EPSILON {
        properties.push("Volume");
    }

    if (previous.rate - next.rate).abs() > f64::EPSILON {
        properties.push("Rate");
    }

    if previous.repeat_mode != next.repeat_mode {
        properties.push("LoopStatus");
    }

    if previous.shuffle != next.shuffle {
        properties.push("Shuffle");
    }

    properties
}

pub(crate) fn mpris_seeked_position(previous: &MprisSnapshot, next: &MprisSnapshot) -> Option<i64> {
    let same_media = previous.has_media
        && next.has_media
        && previous.title == next.title
        && previous.uri == next.uri
        && previous.duration_us == next.duration_us;
    if !same_media {
        return None;
    }

    let delta = (previous.position_us - next.position_us).abs();
    (delta >= MPRIS_SEEKED_DELTA_US).then_some(next.position_us)
}

pub(crate) fn mpris_volume_to_mpv_percent(volume: f64) -> Option<f64> {
    volume
        .is_finite()
        .then(|| (volume * 100.0).clamp(0.0, 130.0))
}

pub(crate) fn mpris_rate_to_mpv_speed(rate: f64) -> Option<f64> {
    rate.is_finite().then(|| rate.clamp(0.25, 4.0))
}

pub(crate) fn mpris_loop_status(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "None",
        RepeatMode::One => "Track",
        RepeatMode::All => "Playlist",
    }
}

pub(crate) fn mpris_repeat_mode(status: &str) -> Option<RepeatMode> {
    match status {
        "None" => Some(RepeatMode::Off),
        "Track" => Some(RepeatMode::One),
        "Playlist" => Some(RepeatMode::All),
        _ => None,
    }
}

pub(crate) fn mpris_snapshot_from_state(
    state: &PlayerState,
    playback: Option<PlaybackState>,
) -> MprisSnapshot {
    let has_media = has_loaded_media_state(state);
    let (title, uri, art_url) = mpris_title_uri_and_art(state);
    let playback = playback.unwrap_or_default();
    let duration_us = playback
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(secs_to_mpris_us);
    let tracklist = mpris_tracklist_from_state(state, duration_us);
    let current_track_id = tracklist
        .iter()
        .find(|track| {
            track
                .uri
                .as_ref()
                .is_some_and(|track_uri| uri.as_ref() == Some(track_uri))
        })
        .map(|track| track.id.clone());
    let track_id = current_track_id.clone().unwrap_or_else(mpris_track_id);

    MprisSnapshot {
        has_media,
        paused: playback.paused || !has_media,
        position_us: playback
            .time_pos
            .filter(|position| position.is_finite() && *position > 0.0)
            .map(secs_to_mpris_us)
            .unwrap_or(0),
        duration_us,
        volume: playback.volume.unwrap_or(100.0).max(0.0) / 100.0,
        rate: playback.speed.unwrap_or(1.0).clamp(0.25, 4.0),
        repeat_mode: state.playlist.repeat(),
        shuffle: state.playlist.shuffle(),
        can_go_next: state.playlist.len() > 1,
        can_go_previous: state.playlist.len() > 1,
        track_id,
        title,
        uri,
        art_url: has_media.then_some(art_url).flatten(),
        tracklist,
        current_track_id,
    }
}

pub(crate) fn mpris_tracklist_from_state(
    state: &PlayerState,
    current_duration_us: Option<i64>,
) -> Vec<MprisTrack> {
    let items = mpris_tracklist_items_from_state(state);
    if items.is_empty() {
        return Vec::new();
    }

    let current_index = mpris_current_tracklist_index(state, &items).unwrap_or(0);
    let (start, end) = mpris_tracklist_window(items.len(), current_index);
    items
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|(index, item)| {
            let id = mpris_tracklist_id_for_item(index, item);
            let uri = mpris_playlist_item_uri(item);
            let is_current = index == current_index;
            MprisTrack {
                id,
                title: item.display_name(),
                uri,
                duration_us: is_current.then_some(current_duration_us).flatten(),
                art_url: mpris_playlist_item_art_url(item),
            }
        })
        .collect()
}

pub(crate) fn mpris_tracklist_items_from_state(state: &PlayerState) -> Vec<PlaylistItem> {
    if !state.playlist.is_empty() {
        return state.playlist.items().to_vec();
    }

    if let Some(path) = state.current_file.as_ref() {
        return vec![PlaylistItem::Local(path.clone())];
    }

    if let Some(url) = state.current_url.as_ref() {
        return vec![PlaylistItem::Url(url.clone())];
    }

    Vec::new()
}

pub(crate) fn mpris_current_tracklist_index(
    state: &PlayerState,
    items: &[PlaylistItem],
) -> Option<usize> {
    items.iter().position(|item| {
        item.is_current(state.current_file.as_deref(), state.current_url.as_deref())
    })
}

pub(crate) fn mpris_tracklist_window(len: usize, current_index: usize) -> (usize, usize) {
    if len <= MPRIS_TRACKLIST_CONTEXT_LIMIT {
        return (0, len);
    }

    let half = MPRIS_TRACKLIST_CONTEXT_LIMIT / 2;
    let start = current_index
        .saturating_sub(half)
        .min(len.saturating_sub(MPRIS_TRACKLIST_CONTEXT_LIMIT));
    (start, start + MPRIS_TRACKLIST_CONTEXT_LIMIT)
}

pub(crate) fn mpris_tracklist_id_for_item(index: usize, item: &PlaylistItem) -> OwnedObjectPath {
    let hash = mpris_playlist_item_hash(item);
    format!("/org/mpris/MediaPlayer2/TrackList/Track/t{index}_{hash:016x}")
        .try_into()
        .expect("generated MPRIS track id should be an object path")
}

pub(crate) fn mpris_tracklist_target_for_id(
    state: &PlayerState,
    track_id: &str,
) -> Option<(usize, PlaylistItem)> {
    mpris_tracklist_items_from_state(state)
        .into_iter()
        .enumerate()
        .find(|(index, item)| mpris_tracklist_id_for_item(*index, item).as_str() == track_id)
}

pub(crate) fn mpris_playlist_item_hash(item: &PlaylistItem) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    };

    match item {
        PlaylistItem::Local(path) => {
            mix(b'L');
            for byte in path.to_string_lossy().as_bytes() {
                mix(*byte);
            }
        }
        PlaylistItem::Url(url) => {
            mix(b'U');
            for byte in url.as_bytes() {
                mix(*byte);
            }
        }
    }

    hash
}

pub(crate) fn mpris_playlist_item_uri(item: &PlaylistItem) -> Option<String> {
    match item {
        PlaylistItem::Local(path) => Some(local_file_uri(path)),
        PlaylistItem::Url(url) => Some(url.clone()),
    }
}

pub(crate) fn mpris_playlist_item_art_url(item: &PlaylistItem) -> Option<String> {
    match item {
        PlaylistItem::Local(path) => mpris_local_art_url(path),
        PlaylistItem::Url(_) => mpris_app_icon_art_url(),
    }
}

pub(crate) fn mpris_title_uri_and_art(
    state: &PlayerState,
) -> (String, Option<String>, Option<String>) {
    if let Some(path) = state.current_file.as_ref() {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        let uri = local_file_uri(path);
        let art_url = mpris_local_art_url(path);
        return (title, Some(uri), art_url);
    }

    if let Some(url) = state.current_url.as_ref() {
        return (
            url.to_owned(),
            Some(url.to_owned()),
            mpris_app_icon_art_url(),
        );
    }

    ("OK Player".to_owned(), None, None)
}

pub(crate) fn local_file_uri(path: &Path) -> String {
    gtk::gio::File::for_path(path).uri().to_string()
}

pub(crate) fn mpris_local_art_url(media_path: &Path) -> Option<String> {
    mpris_sidecar_art_url(media_path)
        .or_else(|| mpris_embedded_art_url(media_path))
        .or_else(mpris_app_icon_art_url)
}

pub(crate) fn mpris_sidecar_art_url(media_path: &Path) -> Option<String> {
    let cache = MPRIS_SIDECAR_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(cached) = cache.get(media_path) {
            return cached.clone();
        }
        let resolved = mpris_sidecar_art_path(media_path).map(|path| local_file_uri(&path));
        cache.insert(media_path.to_path_buf(), resolved.clone());
        return resolved;
    }

    mpris_sidecar_art_path(media_path).map(|path| local_file_uri(&path))
}

pub(crate) fn mpris_embedded_art_url(media_path: &Path) -> Option<String> {
    if !media_formats::is_audio(media_path) {
        return None;
    }

    let key = mpris_embedded_art_cache_key(media_path)?;
    let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };

    match cache.get(&key) {
        Some(MprisEmbeddedArtCacheEntry::Pending) => return None,
        Some(MprisEmbeddedArtCacheEntry::Ready(path)) => {
            return path.as_ref().map(|path| local_file_uri(path));
        }
        None => {}
    }

    cache.insert(key.clone(), MprisEmbeddedArtCacheEntry::Pending);
    drop(cache);
    spawn_mpris_embedded_art_extraction(key);
    None
}

pub(crate) fn spawn_mpris_embedded_art_extraction(key: MprisEmbeddedArtCacheKey) {
    let thread_key = key.clone();
    let spawn_result = thread::Builder::new()
        .name("okp-mpris-art".to_owned())
        .spawn(move || {
            let resolved = mpris_extract_embedded_art_path(&thread_key);
            let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
            if let Ok(mut cache) = cache.lock() {
                cache.insert(thread_key, MprisEmbeddedArtCacheEntry::Ready(resolved));
            }
        });

    if let Err(error) = spawn_result {
        eprintln!("Failed to spawn MPRIS embedded artwork extraction: {error}");
        let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, MprisEmbeddedArtCacheEntry::Ready(None));
        }
    }
}

pub(crate) fn mpris_extract_embedded_art_path(key: &MprisEmbeddedArtCacheKey) -> Option<PathBuf> {
    let output = mpris_embedded_art_cache_path(key);
    if output.is_file() {
        if mpris_has_image_header(&output) {
            return Some(output);
        }
        let _ = fs::remove_file(&output);
    }

    let parent = output.parent()?;
    fs::create_dir_all(parent).ok()?;
    let temp = mpris_embedded_art_temp_path(&output)?;
    let _ = fs::remove_file(&temp);

    let ffmpeg = find_executable("ffmpeg")?;
    let mut child = Command::new(ffmpeg)
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(&key.path)
        .args(["-map", "0:v:0", "-frames:v", "1", "-an", "-sn", "-dn"])
        .arg(&temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let status = wait_for_child_with_timeout(&mut child, MPRIS_EMBEDDED_ART_TIMEOUT).ok()?;
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&temp);
        return None;
    };
    if !status.success() || !mpris_has_image_header(&temp) {
        let _ = fs::remove_file(&temp);
        return None;
    }

    fs::rename(&temp, &output).ok()?;
    Some(output)
}

pub(crate) fn mpris_embedded_art_cache_key(media_path: &Path) -> Option<MprisEmbeddedArtCacheKey> {
    let metadata = fs::metadata(media_path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    Some(MprisEmbeddedArtCacheKey {
        path: media_path.to_path_buf(),
        len: metadata.len(),
        modified_ns,
    })
}

pub(crate) fn mpris_embedded_art_cache_path(key: &MprisEmbeddedArtCacheKey) -> PathBuf {
    mpris_embedded_art_cache_path_in_dir(key, &mpris_embedded_art_cache_dir())
}

pub(crate) fn mpris_embedded_art_cache_path_in_dir(
    key: &MprisEmbeddedArtCacheKey,
    dir: &Path,
) -> PathBuf {
    dir.join(format!("{:016x}.png", mpris_embedded_art_cache_hash(key)))
}

pub(crate) fn mpris_embedded_art_cache_hash(key: &MprisEmbeddedArtCacheKey) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    };

    for byte in key.path.to_string_lossy().as_bytes() {
        mix(*byte);
    }
    for byte in key.len.to_le_bytes() {
        mix(byte);
    }
    for byte in key.modified_ns.to_le_bytes() {
        mix(byte);
    }

    hash
}

pub(crate) fn mpris_embedded_art_cache_dir() -> PathBuf {
    if let Some(cache_dir) =
        env::var_os("OKP_MPRIS_ART_CACHE_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(cache_dir);
    }
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(cache_home).join("ok-player/mpris-art");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".cache/ok-player/mpris-art");
    }
    env::temp_dir().join("ok-player/mpris-art")
}

pub(crate) fn mpris_embedded_art_temp_path(output: &Path) -> Option<PathBuf> {
    let stem = output.file_stem()?.to_string_lossy();
    Some(output.with_file_name(format!("{stem}.part.{}.png", std::process::id())))
}

pub(crate) fn mpris_sidecar_art_path(media_path: &Path) -> Option<PathBuf> {
    let dir = media_path.parent()?;
    let media_stem = media_path.file_stem()?.to_str()?;
    let mut candidates: Vec<(i32, usize, PathBuf)> = Vec::new();

    for entry in fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(extension_rank) = mpris_art_extension_rank(&path) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let slot = if stem.eq_ignore_ascii_case(media_stem) {
            -1
        } else if let Some(index) = mpris_folder_art_stem_index(stem) {
            index as i32
        } else {
            continue;
        };
        if !mpris_has_image_header(&path) {
            continue;
        }
        candidates.push((slot, extension_rank, path));
    }

    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    candidates.into_iter().map(|(_, _, path)| path).next()
}

pub(crate) fn mpris_art_extension_rank(path: &Path) -> Option<usize> {
    let extension = path.extension()?.to_str()?;
    MPRIS_ART_EXTENSIONS
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
}

pub(crate) fn mpris_folder_art_stem_index(stem: &str) -> Option<usize> {
    MPRIS_FOLDER_ART_STEMS
        .iter()
        .position(|candidate| stem.eq_ignore_ascii_case(candidate))
}

pub(crate) fn mpris_has_image_header(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = [0_u8; 12];
    if file.read_exact(&mut bytes).is_err() {
        return false;
    }

    (bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff)
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

pub(crate) fn mpris_app_icon_art_url() -> Option<String> {
    MPRIS_APP_ICON_ART_URL
        .get_or_init(|| mpris_app_icon_art_path().map(|path| local_file_uri(&path)))
        .clone()
}

pub(crate) fn mpris_app_icon_art_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(
        "/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg",
    ));
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("com.befeast.okplayer.svg"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.svg"),
    );

    candidates.into_iter().find(|path| path.is_file())
}

pub(crate) fn secs_to_mpris_us(seconds: f64) -> i64 {
    (seconds.max(0.0) * 1_000_000.0).round() as i64
}

pub(crate) fn mpris_metadata(snapshot: &MprisSnapshot) -> HashMap<String, OwnedValue> {
    mpris_metadata_map(
        snapshot.track_id.clone(),
        &snapshot.title,
        snapshot.uri.as_deref(),
        snapshot.duration_us,
        snapshot.art_url.as_deref(),
    )
}

pub(crate) fn mpris_track_metadata(track: &MprisTrack) -> HashMap<String, OwnedValue> {
    mpris_metadata_map(
        track.id.clone(),
        &track.title,
        track.uri.as_deref(),
        track.duration_us,
        track.art_url.as_deref(),
    )
}

pub(crate) fn mpris_metadata_map(
    track_id: OwnedObjectPath,
    title: &str,
    uri: Option<&str>,
    duration_us: Option<i64>,
    art_url: Option<&str>,
) -> HashMap<String, OwnedValue> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "mpris:trackid".to_owned(),
        Value::from(track_id).try_into().expect("track id value"),
    );
    metadata.insert(
        "xesam:title".to_owned(),
        Value::from(title).try_into().expect("title value"),
    );
    if let Some(duration_us) = duration_us {
        metadata.insert(
            "mpris:length".to_owned(),
            Value::from(duration_us).try_into().expect("length value"),
        );
    }
    if let Some(uri) = uri {
        metadata.insert(
            "xesam:url".to_owned(),
            Value::from(uri).try_into().expect("url value"),
        );
    }
    if let Some(art_url) = art_url {
        metadata.insert(
            "mpris:artUrl".to_owned(),
            Value::from(art_url).try_into().expect("art url value"),
        );
    }
    metadata
}

pub(crate) fn mpris_track_id() -> OwnedObjectPath {
    MPRIS_TRACK_PATH
        .try_into()
        .expect("static MPRIS track path")
}

pub(crate) fn mpris_no_track_id() -> OwnedObjectPath {
    MPRIS_TRACKLIST_NO_TRACK_PATH
        .try_into()
        .expect("static MPRIS no-track path")
}
