//! Background event pump for the libmpv client API.
//!
//! The GTK shell used to poll mpv from the GLib main context every 200 ms —
//! a synchronous `mpv_get_property` burst on the thread that drives the UI, the
//! exact deadlock class the #133 tripwire was built to catch. This module
//! retires that poll: a dedicated background thread owns event reception
//! (`mpv_set_wakeup_callback` wakes it, `mpv_wait_event` drains it) and reads
//! every observed property off the render/UI thread, publishing the result into
//! a lock-guarded [`Snapshot`]. The shell then projects that snapshot onto its
//! widgets without ever touching mpv from the main context — the reads become
//! plain in-memory lookups, so the guard stays green.
//!
//! Lifecycle and requested async-command events are queued for the shell to
//! drain in order; property changes only ever mutate the snapshot.

use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use libc::{c_char, c_void};

use crate::ffi;
use crate::player::{
    AbLoopState, AudioDevice, Chapter, EndFileReason, MediaInfo, MpvEvent, PlaybackDiagnostics,
    PlaybackState, RawReader, Track, VideoDimensions, WaylandPresentationFeedback,
    audio_device_selected, audio_devices_from_entries, end_file_reason, media_info_with_source,
};

/// Properties the pump observes so mpv wakes it (and refreshes the snapshot)
/// whenever any of them changes. Most use `MPV_FORMAT_NONE` and are re-read on
/// the background thread. Audio devices deliberately consume typed event
/// payloads: querying that backend synchronously can block during PipeWire
/// teardown and must never hold the main event pump hostage.
const OBSERVED_PROPERTIES: &[(&str, libc::c_int)] = &[
    // Playback scalars projected onto the transport bar.
    ("time-pos", ffi::MPV_FORMAT_NONE),
    ("duration", ffi::MPV_FORMAT_NONE),
    ("pause", ffi::MPV_FORMAT_NONE),
    ("volume", ffi::MPV_FORMAT_NONE),
    ("speed", ffi::MPV_FORMAT_NONE),
    ("demuxer-cache-duration", ffi::MPV_FORMAT_NONE),
    // Container frame rate for the seek/frame-step readout; changes on load.
    ("container-fps", ffi::MPV_FORMAT_NONE),
    // Live acceptance diagnostics. Reads remain on the pump thread; the GTK
    // main context only consumes the published snapshot.
    ("hwdec-current", ffi::MPV_FORMAT_NONE),
    ("decoder-frame-drop-count", ffi::MPV_FORMAT_NONE),
    ("frame-drop-count", ffi::MPV_FORMAT_NONE),
    // Subtitle state surfaced in the subtitle popover / saved preferences.
    ("sub-delay", ffi::MPV_FORMAT_NONE),
    ("sub-scale", ffi::MPV_FORMAT_NONE),
    ("secondary-sid", ffi::MPV_FORMAT_NONE),
    // Audio sync offset surfaced in the audio popover / saved preferences.
    ("audio-delay", ffi::MPV_FORMAT_NONE),
    // A-B loop endpoints surfaced on the timeline.
    ("ab-loop-a", ffi::MPV_FORMAT_NONE),
    ("ab-loop-b", ffi::MPV_FORMAT_NONE),
    // Lists that only change on load / track or device selection.
    ("track-list", ffi::MPV_FORMAT_NONE),
    ("chapter-list", ffi::MPV_FORMAT_NONE),
    ("audio-device-list", ffi::MPV_FORMAT_NODE),
    ("audio-device", ffi::MPV_FORMAT_STRING),
];

const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_millis(250);
const SHUTDOWN_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The observed state the shell projects onto its widgets. Cloned out of the
/// pump under a short lock; never holds an mpv handle.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) playback: PlaybackState,
    pub(crate) playback_diagnostics: PlaybackDiagnostics,
    pub(crate) ab_loop: AbLoopState,
    pub(crate) subtitle_delay: f64,
    pub(crate) subtitle_scale: f64,
    pub(crate) audio_delay: f64,
    pub(crate) speed: f64,
    pub(crate) secondary_subtitle_id: Option<i64>,
    pub(crate) video_dimensions: Option<VideoDimensions>,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) tracks: Vec<Track>,
    pub(crate) audio_devices: Vec<AudioDevice>,
    pub(crate) media_info: Option<MediaInfo>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            playback: PlaybackState::default(),
            playback_diagnostics: PlaybackDiagnostics::default(),
            ab_loop: AbLoopState::default(),
            subtitle_delay: 0.0,
            audio_delay: 0.0,
            // Scale and speed report as 1.0 (100 % / normal) before mpv has a
            // value, matching the shell's historical `unwrap_or(1.0)` fallbacks.
            subtitle_scale: 1.0,
            speed: 1.0,
            secondary_subtitle_id: None,
            video_dimensions: None,
            chapters: Vec::new(),
            tracks: Vec::new(),
            audio_devices: Vec::new(),
            media_info: None,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct RecomputeFlags {
    tracks: bool,
    chapters: bool,
    media_info: bool,
}

impl RecomputeFlags {
    fn all() -> Self {
        Self {
            tracks: true,
            chapters: true,
            // No media is loaded at startup, so a full metadata walk here only
            // adds dozens of blocking reads before the first useful event.
            media_info: false,
        }
    }
}

/// State shared between the shell (main thread), the mpv wakeup callback (a
/// foreign mpv thread), and the pump thread. `RawReader` is a bare handle
/// pointer, so the whole struct is only `Send`/`Sync` by our own guarantee that
/// the libmpv client API is thread-safe.
struct PumpShared {
    reader: RawReader,
    snapshot: Mutex<Snapshot>,
    events: Mutex<Vec<MpvEvent>>,
    wayland_presentation_feedback: Mutex<Vec<WaylandPresentationFeedback>>,
    diagnostic_messages: Mutex<VecDeque<String>>,
    media_source: Mutex<Option<PathBuf>>,
    audio_device_current: Mutex<String>,
    wake: Mutex<bool>,
    condvar: Condvar,
    running: AtomicBool,
    codec_failure_reported: AtomicBool,
}

pub(crate) struct EventPump {
    handle: NonNull<ffi::mpv_handle>,
    shared: Arc<PumpShared>,
    join: Option<JoinHandle<()>>,
}

impl EventPump {
    /// Register the observed properties and the wakeup callback, then spawn the
    /// background pump. Must be called on the thread that owns `handle` (the UI
    /// thread) after `mpv_initialize`.
    pub(crate) fn start(handle: NonNull<ffi::mpv_handle>) -> Self {
        Self::start_with_audio_devices(handle, true)
    }

    pub(crate) fn start_without_audio_devices(handle: NonNull<ffi::mpv_handle>) -> Self {
        Self::start_with_audio_devices(handle, false)
    }

    fn start_with_audio_devices(
        handle: NonNull<ffi::mpv_handle>,
        observe_audio_devices: bool,
    ) -> Self {
        let shared = Arc::new(PumpShared {
            reader: RawReader::new(handle),
            snapshot: Mutex::new(Snapshot::default()),
            events: Mutex::new(Vec::new()),
            wayland_presentation_feedback: Mutex::new(Vec::new()),
            diagnostic_messages: Mutex::new(VecDeque::new()),
            media_source: Mutex::new(None),
            audio_device_current: Mutex::new("auto".to_owned()),
            // Start "pending" so the pump populates the snapshot immediately.
            wake: Mutex::new(true),
            condvar: Condvar::new(),
            running: AtomicBool::new(true),
            codec_failure_reported: AtomicBool::new(false),
        });

        for (name, format) in OBSERVED_PROPERTIES {
            if !observe_audio_devices && matches!(*name, "audio-device-list" | "audio-device") {
                continue;
            }
            let cname = CString::new(*name).expect("observed property names never contain nul");
            unsafe {
                ffi::mpv_observe_property(handle.as_ptr(), 0, cname.as_ptr(), *format);
            }
        }

        // The callback ctx borrows the Arc allocation; it stays valid because
        // `self.shared` outlives the callback (we unset it before dropping).
        unsafe {
            ffi::mpv_set_wakeup_callback(
                handle.as_ptr(),
                Some(wakeup_trampoline),
                Arc::as_ptr(&shared) as *mut c_void,
            );
        }

        let thread_shared = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("okp-mpv-pump".to_owned())
            .spawn(move || pump_loop(&thread_shared))
            .expect("spawning the mpv event pump thread must succeed");

        Self {
            handle,
            shared,
            join: Some(join),
        }
    }

    pub(crate) fn playback_state(&self) -> PlaybackState {
        lock(&self.shared.snapshot).playback
    }

    pub(crate) fn playback_diagnostics(&self) -> PlaybackDiagnostics {
        lock(&self.shared.snapshot).playback_diagnostics.clone()
    }

    pub(crate) fn take_wayland_presentation_feedback(&self) -> Vec<WaylandPresentationFeedback> {
        std::mem::take(&mut *lock(&self.shared.wayland_presentation_feedback))
    }

    pub(crate) fn ab_loop_state(&self) -> AbLoopState {
        lock(&self.shared.snapshot).ab_loop
    }

    pub(crate) fn subtitle_delay(&self) -> f64 {
        lock(&self.shared.snapshot).subtitle_delay
    }

    pub(crate) fn audio_delay(&self) -> f64 {
        lock(&self.shared.snapshot).audio_delay
    }

    pub(crate) fn subtitle_scale(&self) -> f64 {
        lock(&self.shared.snapshot).subtitle_scale
    }

    pub(crate) fn speed(&self) -> f64 {
        lock(&self.shared.snapshot).speed
    }

    pub(crate) fn secondary_subtitle_id(&self) -> Option<i64> {
        lock(&self.shared.snapshot).secondary_subtitle_id
    }

    pub(crate) fn video_dimensions(&self) -> Option<VideoDimensions> {
        lock(&self.shared.snapshot).video_dimensions
    }

    pub(crate) fn chapters(&self) -> Vec<Chapter> {
        lock(&self.shared.snapshot).chapters.clone()
    }

    pub(crate) fn tracks(&self) -> Vec<Track> {
        lock(&self.shared.snapshot).tracks.clone()
    }

    pub(crate) fn audio_devices(&self) -> Vec<AudioDevice> {
        lock(&self.shared.snapshot).audio_devices.clone()
    }

    pub(crate) fn media_info(&self) -> Option<MediaInfo> {
        lock(&self.shared.snapshot).media_info.clone()
    }

    pub(crate) fn begin_media_load(&self) {
        let mut snapshot = lock(&self.shared.snapshot);
        snapshot.video_dimensions = None;
        snapshot.media_info = None;
        drop(snapshot);
        lock(&self.shared.diagnostic_messages).clear();
        self.shared
            .codec_failure_reported
            .store(false, Ordering::Release);
    }

    /// Drain the lifecycle events queued since the last call, oldest first.
    pub(crate) fn take_lifecycle_events(&self) -> Vec<MpvEvent> {
        std::mem::take(&mut *lock(&self.shared.events))
    }

    /// Record the local path backing the current media so `media-info` reports
    /// the same title/path the shell would have passed synchronously. Source
    /// identity is a local projection, so update it without another libmpv
    /// metadata walk; the next `FileLoaded` refresh fills the engine fields.
    pub(crate) fn set_media_source(&self, source: Option<PathBuf>) {
        let mut stored_source = lock(&self.shared.media_source);
        *stored_source = source;
        let mut snapshot = lock(&self.shared.snapshot);
        snapshot.media_info = stored_source
            .as_deref()
            .map(|source| media_info_with_source(snapshot.media_info.take(), source));
    }

    /// Stop the wakeup callback and wake the pump so it observes the shutdown
    /// flag. A prompt worker is joined here; a worker blocked inside libmpv is
    /// returned to the owner so handle destruction can be deferred without
    /// blocking the caller or racing the in-flight API call.
    pub(crate) fn shutdown(mut self) -> Option<JoinHandle<()>> {
        unsafe {
            ffi::mpv_set_wakeup_callback(self.handle.as_ptr(), None, ptr::null_mut());
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.condvar.notify_all();
        let blocked = self
            .join
            .take()
            .and_then(|join| join_with_timeout(join, SHUTDOWN_JOIN_TIMEOUT).err());
        if blocked.is_some() {
            eprintln!("[okp-mpv] event pump exceeded the shutdown deadline; deferring teardown");
        }
        blocked
    }
}

fn join_with_timeout(join: JoinHandle<()>, timeout: Duration) -> Result<(), JoinHandle<()>> {
    let deadline = Instant::now() + timeout;
    while !join.is_finished() {
        let now = Instant::now();
        if now >= deadline {
            return Err(join);
        }
        thread::sleep((deadline - now).min(SHUTDOWN_JOIN_POLL_INTERVAL));
    }
    let _ = join.join();
    Ok(())
}

/// Wakeup callback invoked by mpv on a foreign thread whenever events are
/// pending. It only flips the pending flag and signals the pump — no client API
/// calls, per the mpv contract.
unsafe extern "C" fn wakeup_trampoline(ctx: *mut c_void) {
    let Some(shared) = (unsafe { (ctx as *const PumpShared).as_ref() }) else {
        return;
    };
    *lock(&shared.wake) = true;
    shared.condvar.notify_one();
}

fn pump_loop(shared: &Arc<PumpShared>) {
    // Populate the snapshot once so observed reads return real data promptly.
    recompute(shared, RecomputeFlags::all());

    while shared.running.load(Ordering::Acquire) {
        {
            let mut pending = lock(&shared.wake);
            while !*pending && shared.running.load(Ordering::Acquire) {
                pending = shared
                    .condvar
                    .wait(pending)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            *pending = false;
        }
        if !shared.running.load(Ordering::Acquire) {
            break;
        }

        let (lifecycle, flags) = drain_events(shared);
        // Refresh the snapshot *before* the lifecycle events become visible, so
        // a shell handler that reacts to `FileLoaded` already sees fresh tracks.
        recompute(shared, flags);
        if !lifecycle.is_empty() {
            lock(&shared.events).extend(lifecycle);
        }
    }
}

fn drain_events(shared: &Arc<PumpShared>) -> (Vec<MpvEvent>, RecomputeFlags) {
    let mut lifecycle = Vec::new();
    let mut flags = RecomputeFlags::default();
    let handle = shared.reader.handle().as_ptr();

    loop {
        let event = unsafe { ffi::mpv_wait_event(handle, 0.0) };
        let Some(event) = (unsafe { event.as_ref() }) else {
            break;
        };

        match event.event_id {
            ffi::MPV_EVENT_NONE => break,
            ffi::MPV_EVENT_SHUTDOWN => {
                lifecycle.push(MpvEvent::Shutdown);
                shared.running.store(false, Ordering::Release);
            }
            ffi::MPV_EVENT_LOG_MESSAGE => {
                if let Some(feedback) = wayland_presentation_feedback(event) {
                    lock(&shared.wayland_presentation_feedback).push(feedback);
                    continue;
                }
                if let Some(message) = log_message(event) {
                    {
                        let mut messages = lock(&shared.diagnostic_messages);
                        if messages.len() == 24 {
                            messages.pop_front();
                        }
                        messages.push_back(message.clone());
                    }
                    if okp_core::playback_failure::is_mpv_codec_failure(&message)
                        && !shared.codec_failure_reported.swap(true, Ordering::AcqRel)
                    {
                        // Bind the warning to the engine source while still on
                        // the pump thread. The GTK drain may run after the user
                        // has already opened another source.
                        let path = shared.reader.path();
                        lifecycle.push(MpvEvent::DecoderFailed {
                            path,
                            diagnostic_messages: lock(&shared.diagnostic_messages)
                                .iter()
                                .cloned()
                                .collect(),
                        });
                    }
                }
            }
            ffi::MPV_EVENT_FILE_LOADED => {
                let video_dimensions = shared.reader.video_dimensions().ok().flatten();
                lock(&shared.snapshot).video_dimensions = video_dimensions;
                // A successful load finalizes the previous source; discard any
                // stale log messages so they cannot be misattributed to a later
                // failure. Logs from the currently ending source are captured
                // when `EndFile` drains the buffer.
                lock(&shared.diagnostic_messages).clear();
                lifecycle.push(MpvEvent::FileLoaded { video_dimensions });
                flags.tracks = true;
                flags.chapters = true;
                flags.media_info = true;
            }
            ffi::MPV_EVENT_VIDEO_RECONFIG => {
                let video_dimensions = shared.reader.video_dimensions().ok().flatten();
                if video_dimensions.is_some() {
                    lock(&shared.snapshot).video_dimensions = video_dimensions;
                }
                lifecycle.push(MpvEvent::VideoReconfig { video_dimensions });
                flags.media_info = true;
            }
            ffi::MPV_EVENT_END_FILE => {
                let reason = if let Some(end_file) =
                    unsafe { event.data.cast::<ffi::mpv_event_end_file>().as_ref() }
                {
                    end_file_reason(end_file.reason, end_file.error)
                } else {
                    EndFileReason::Unknown(event.error)
                };
                // Snapshot the ended source's path/URL while the pump is on the reader
                // thread (a blocking mpv read here is allowed). The shell compares it to
                // the current source when draining, so a stale `EndFile::Error` whose
                // source was superseded between the engine firing the event and the next
                // poll is dropped instead of failing the new source.
                let path = shared.reader.path();
                let diagnostic_messages = lock(&shared.diagnostic_messages).drain(..).collect();
                lifecycle.push(MpvEvent::EndFile {
                    reason,
                    path,
                    diagnostic_messages,
                });
            }
            ffi::MPV_EVENT_PROPERTY_CHANGE => {
                if let Some(property) =
                    unsafe { event.data.cast::<ffi::mpv_event_property>().as_ref() }
                    && !property.name.is_null()
                {
                    match unsafe { CStr::from_ptr(property.name) }.to_bytes() {
                        b"track-list" => {
                            flags.tracks = true;
                            flags.media_info = true;
                        }
                        b"chapter-list" => {
                            flags.chapters = true;
                            flags.media_info = true;
                        }
                        b"audio-device-list" => {
                            if let Some(entries) = unsafe { audio_device_entries(property) } {
                                let current = lock(&shared.audio_device_current).clone();
                                lock(&shared.snapshot).audio_devices =
                                    audio_devices_from_entries(entries, &current);
                            }
                        }
                        b"audio-device" => {
                            if let Some(current) = unsafe { property_string(property) } {
                                *lock(&shared.audio_device_current) = current.clone();
                                for device in &mut lock(&shared.snapshot).audio_devices {
                                    device.selected = audio_device_selected(&device.name, &current);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ffi::MPV_EVENT_COMMAND_REPLY if event.reply_userdata != 0 => {
                lifecycle.push(MpvEvent::CommandReply {
                    request_id: event.reply_userdata,
                    error: event.error,
                });
            }
            ffi::MPV_EVENT_COMMAND_REPLY if event.error < 0 => {
                eprintln!("[okp-mpv] async command failed with code {}", event.error);
            }
            _ => {}
        }
    }

    (lifecycle, flags)
}

fn wayland_presentation_feedback(event: &ffi::mpv_event) -> Option<WaylandPresentationFeedback> {
    let message = unsafe { event.data.cast::<ffi::mpv_event_log_message>().as_ref() }?;
    let text = c_string(message.text);
    let payload = text.trim().strip_prefix("okp-wayland-embed-feedback ")?;
    if payload == "discarded" {
        return Some(WaylandPresentationFeedback::Discarded {
            observed_monotonic_ns: monotonic_absolute_ns(),
        });
    }
    let payload = payload.strip_prefix("presented ")?;
    let mut presented_ns = None;
    let mut refresh_ns = None;
    let mut sequence = None;
    let mut flags = None;
    let mut width = None;
    let mut height = None;
    for field in payload.split_whitespace() {
        let (name, value) = field.split_once('=')?;
        match name {
            "presented_ns" => presented_ns = value.parse().ok(),
            "refresh_ns" => refresh_ns = value.parse().ok(),
            "sequence" => sequence = value.parse().ok(),
            "flags" => flags = value.parse().ok(),
            "width" => width = value.parse().ok(),
            "height" => height = value.parse().ok(),
            _ => return None,
        }
    }
    Some(WaylandPresentationFeedback::Presented {
        observed_monotonic_ns: monotonic_absolute_ns(),
        presented_ns: presented_ns?,
        refresh_ns: refresh_ns?,
        sequence: sequence?,
        flags: flags?,
        width: width?,
        height: height?,
    })
}

fn monotonic_absolute_ns() -> u64 {
    let mut timestamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut timestamp) } != 0 {
        return 0;
    }
    u64::try_from(timestamp.tv_sec)
        .unwrap_or_default()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(timestamp.tv_nsec).unwrap_or_default())
}

fn log_message(event: &ffi::mpv_event) -> Option<String> {
    let message = unsafe { event.data.cast::<ffi::mpv_event_log_message>().as_ref() }?;
    let prefix = c_string(message.prefix);
    let text = c_string(message.text);
    if text.is_empty() {
        return None;
    }
    let combined = if prefix.is_empty() {
        text
    } else {
        format!("{prefix}: {text}")
    };
    Some(combined.trim().chars().take(512).collect())
}

fn c_string(pointer: *const libc::c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Read the current values off the background thread and publish them. Scalars
/// are cheap and refreshed on every wakeup; the heavier lists are only re-read
/// when a change flagged them, so playback ticks stay lightweight.
fn recompute(shared: &Arc<PumpShared>, flags: RecomputeFlags) {
    let reader = shared.reader;

    let playback = reader.playback_state().unwrap_or_default();
    let playback_diagnostics = reader.playback_diagnostics().unwrap_or_default();
    let ab_loop = reader.ab_loop_state().unwrap_or_default();
    let subtitle_delay = reader.subtitle_delay().unwrap_or(0.0);
    let audio_delay = reader.audio_delay().unwrap_or(0.0);
    let subtitle_scale = reader.subtitle_scale().unwrap_or(1.0);
    let speed = reader.speed().unwrap_or(1.0);
    let secondary_subtitle_id = reader.secondary_subtitle_id().unwrap_or_default();

    let tracks = flags.tracks.then(|| reader.tracks().unwrap_or_default());
    {
        let mut snapshot = lock(&shared.snapshot);
        snapshot.playback = playback;
        snapshot.playback_diagnostics = playback_diagnostics;
        snapshot.ab_loop = ab_loop;
        snapshot.subtitle_delay = subtitle_delay;
        snapshot.audio_delay = audio_delay;
        snapshot.subtitle_scale = subtitle_scale;
        snapshot.speed = speed;
        snapshot.secondary_subtitle_id = secondary_subtitle_id;
        if let Some(tracks) = tracks {
            snapshot.tracks = tracks;
        }
    }

    // Publish track changes before the larger chapter/media-info walks. A
    // slower optional metadata field must not hide a subtitle list the pump has
    // already read from the loaded file.
    if flags.chapters {
        let chapters = reader.chapters().unwrap_or_default();
        lock(&shared.snapshot).chapters = chapters;
    }
    if flags.media_info {
        let media_info = reader.media_info(None).ok();
        let source = lock(&shared.media_source);
        lock(&shared.snapshot).media_info = match source.as_deref() {
            Some(source) => Some(media_info_with_source(media_info, source)),
            None => media_info,
        };
    }
}

unsafe fn audio_device_entries(
    property: &ffi::mpv_event_property,
) -> Option<Vec<(String, Option<String>)>> {
    if property.format != ffi::MPV_FORMAT_NODE || property.data.is_null() {
        return None;
    }
    let node = unsafe { property.data.cast::<ffi::mpv_node>().as_ref() }?;
    if node.format != ffi::MPV_FORMAT_NODE_ARRAY {
        return None;
    }
    let list = unsafe { node.value.list.as_ref() }?;
    if list.num < 0 || (list.num > 0 && list.values.is_null()) {
        return None;
    }
    if list.num == 0 {
        return Some(Vec::new());
    }

    let values = unsafe { std::slice::from_raw_parts(list.values, list.num as usize) };
    Some(
        values
            .iter()
            .filter_map(|entry| {
                let name = unsafe { node_map_string(entry, b"name") }?;
                let description = unsafe { node_map_string(entry, b"description") };
                Some((name, description))
            })
            .collect(),
    )
}

unsafe fn node_map_string(node: &ffi::mpv_node, wanted: &[u8]) -> Option<String> {
    if node.format != ffi::MPV_FORMAT_NODE_MAP {
        return None;
    }
    let list = unsafe { node.value.list.as_ref() }?;
    if list.num < 0 || (list.num > 0 && (list.values.is_null() || list.keys.is_null())) {
        return None;
    }
    if list.num == 0 {
        return None;
    }

    let values = unsafe { std::slice::from_raw_parts(list.values, list.num as usize) };
    let keys = unsafe { std::slice::from_raw_parts(list.keys, list.num as usize) };
    for (key, value) in keys.iter().zip(values) {
        if key.is_null() || value.format != ffi::MPV_FORMAT_STRING {
            continue;
        }
        if unsafe { CStr::from_ptr(*key) }.to_bytes() == wanted {
            let string = unsafe { value.value.string };
            if string.is_null() {
                return None;
            }
            return Some(
                unsafe { CStr::from_ptr(string) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    None
}

unsafe fn property_string(property: &ffi::mpv_event_property) -> Option<String> {
    if property.format != ffi::MPV_FORMAT_STRING || property.data.is_null() {
        return None;
    }
    let string = unsafe { *property.data.cast::<*const c_char>() };
    if string.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(string) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// Lock a mutex, recovering the guard if the pump thread poisoned it — a busy
/// core must never turn a pump panic into a cascade of UI-thread panics.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn log_event(text: &CString) -> (ffi::mpv_event_log_message, ffi::mpv_event) {
        let message = ffi::mpv_event_log_message {
            prefix: c"wayland".as_ptr(),
            level: c"warn".as_ptr(),
            text: text.as_ptr(),
            log_level: 2,
        };
        let event = ffi::mpv_event {
            event_id: ffi::MPV_EVENT_LOG_MESSAGE,
            error: 0,
            reply_userdata: 0,
            data: ptr::null_mut(),
        };
        (message, event)
    }

    #[test]
    fn parses_patched_wayland_compositor_feedback_without_treating_it_as_failure_text() {
        let text = CString::new(
            "okp-wayland-embed-feedback presented presented_ns=123456 refresh_ns=16666667 sequence=42 flags=3 width=1920 height=1037\n",
        )
        .unwrap();
        let (mut message, mut event) = log_event(&text);
        event.data = std::ptr::from_mut(&mut message).cast();
        let Some(WaylandPresentationFeedback::Presented {
            observed_monotonic_ns,
            presented_ns,
            refresh_ns,
            sequence,
            flags,
            width,
            height,
        }) = wayland_presentation_feedback(&event)
        else {
            panic!("presented feedback should parse");
        };
        assert!(observed_monotonic_ns > 0);
        assert_eq!(presented_ns, 123456);
        assert_eq!(refresh_ns, 16_666_667);
        assert_eq!(sequence, 42);
        assert_eq!(flags, 3);
        assert_eq!((width, height), (1920, 1037));

        let discarded = CString::new("okp-wayland-embed-feedback discarded\n").unwrap();
        let (mut message, mut event) = log_event(&discarded);
        event.data = std::ptr::from_mut(&mut message).cast();
        let Some(WaylandPresentationFeedback::Discarded {
            observed_monotonic_ns,
        }) = wayland_presentation_feedback(&event)
        else {
            panic!("discarded feedback should parse");
        };
        assert!(observed_monotonic_ns > 0);
    }

    #[test]
    fn typed_audio_device_payloads_are_copied_from_the_event() {
        let name_key = CString::new("name").unwrap();
        let description_key = CString::new("description").unwrap();
        let name = CString::new("pulse/speakers").unwrap();
        let description = CString::new("Speakers").unwrap();
        let mut map_values = [
            ffi::mpv_node {
                value: ffi::mpv_node_value {
                    string: name.as_ptr().cast_mut(),
                },
                format: ffi::MPV_FORMAT_STRING,
            },
            ffi::mpv_node {
                value: ffi::mpv_node_value {
                    string: description.as_ptr().cast_mut(),
                },
                format: ffi::MPV_FORMAT_STRING,
            },
        ];
        let mut map_keys = [
            name_key.as_ptr().cast_mut(),
            description_key.as_ptr().cast_mut(),
        ];
        let mut map = ffi::mpv_node_list {
            num: 2,
            values: map_values.as_mut_ptr(),
            keys: map_keys.as_mut_ptr(),
        };
        let mut array_values = [ffi::mpv_node {
            value: ffi::mpv_node_value {
                list: std::ptr::from_mut(&mut map),
            },
            format: ffi::MPV_FORMAT_NODE_MAP,
        }];
        let mut array = ffi::mpv_node_list {
            num: 1,
            values: array_values.as_mut_ptr(),
            keys: ptr::null_mut(),
        };
        let mut root = ffi::mpv_node {
            value: ffi::mpv_node_value {
                list: std::ptr::from_mut(&mut array),
            },
            format: ffi::MPV_FORMAT_NODE_ARRAY,
        };
        let property = ffi::mpv_event_property {
            name: ptr::null(),
            format: ffi::MPV_FORMAT_NODE,
            data: std::ptr::from_mut(&mut root).cast(),
        };

        assert_eq!(
            unsafe { audio_device_entries(&property) },
            Some(vec![(
                "pulse/speakers".to_owned(),
                Some("Speakers".to_owned())
            )])
        );

        let current = CString::new("pulse/speakers").unwrap();
        let mut current_ptr = current.as_ptr();
        let current_property = ffi::mpv_event_property {
            name: ptr::null(),
            format: ffi::MPV_FORMAT_STRING,
            data: std::ptr::from_mut(&mut current_ptr).cast(),
        };
        assert_eq!(
            unsafe { property_string(&current_property) }.as_deref(),
            Some("pulse/speakers")
        );
    }

    #[test]
    fn blocked_worker_is_returned_before_the_candidate_stall_guard() {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            ready_tx.send(()).expect("test worker should report ready");
            release_rx.recv().expect("test worker should be released");
        });
        ready_rx.recv().expect("test worker should start");

        let started = Instant::now();
        let join = join_with_timeout(join, Duration::from_millis(25))
            .expect_err("a blocked worker must be handed back to the caller");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "bounded shutdown must fail fast instead of reaching the candidate watchdog"
        );

        release_tx.send(()).expect("test worker should be released");
        join.join().expect("test worker should finish cleanly");
    }
}
