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
//! Lifecycle events (`FileLoaded`, `EndFile`, `Shutdown`) are queued for the
//! shell to drain in order; property changes only ever mutate the snapshot.

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use libc::c_void;

use crate::ffi;
use crate::player::{
    AbLoopState, AudioDevice, Chapter, EndFileReason, MediaInfo, MpvEvent, PlaybackState,
    RawReader, Track, end_file_reason,
};

/// Properties the pump observes so mpv wakes it (and refreshes the snapshot)
/// whenever any of them changes. All are observed with `MPV_FORMAT_NONE`: the
/// pump re-reads the current value off the background thread, which keeps the
/// observation registration trivial and avoids decoding node payloads.
const OBSERVED_PROPERTIES: &[&str] = &[
    // Playback scalars projected onto the transport bar.
    "time-pos",
    "duration",
    "pause",
    "volume",
    "speed",
    // Container frame rate for the seek/frame-step readout; changes on load.
    "container-fps",
    // Subtitle state surfaced in the subtitle popover / saved preferences.
    "sub-delay",
    "sub-scale",
    "secondary-sid",
    // A-B loop endpoints surfaced on the timeline.
    "ab-loop-a",
    "ab-loop-b",
    // Lists that only change on load / track or device selection.
    "track-list",
    "chapter-list",
    "audio-device-list",
    "audio-device",
];

/// The observed state the shell projects onto its widgets. Cloned out of the
/// pump under a short lock; never holds an mpv handle.
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) playback: PlaybackState,
    pub(crate) ab_loop: AbLoopState,
    pub(crate) subtitle_delay: f64,
    pub(crate) subtitle_scale: f64,
    pub(crate) speed: f64,
    pub(crate) secondary_subtitle_id: Option<i64>,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) tracks: Vec<Track>,
    pub(crate) audio_devices: Vec<AudioDevice>,
    pub(crate) media_info: Option<MediaInfo>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            playback: PlaybackState::default(),
            ab_loop: AbLoopState::default(),
            subtitle_delay: 0.0,
            // Scale and speed report as 1.0 (100 % / normal) before mpv has a
            // value, matching the shell's historical `unwrap_or(1.0)` fallbacks.
            subtitle_scale: 1.0,
            speed: 1.0,
            secondary_subtitle_id: None,
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
    audio_devices: bool,
    media_info: bool,
}

impl RecomputeFlags {
    fn all() -> Self {
        Self {
            tracks: true,
            chapters: true,
            audio_devices: true,
            media_info: true,
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
    media_source: Mutex<Option<PathBuf>>,
    /// Set when the shell records a new media source so the next pump pass
    /// rebuilds `media_info` against it, even without a fresh mpv event.
    media_info_dirty: AtomicBool,
    wake: Mutex<bool>,
    condvar: Condvar,
    running: AtomicBool,
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
        let shared = Arc::new(PumpShared {
            reader: RawReader::new(handle),
            snapshot: Mutex::new(Snapshot::default()),
            events: Mutex::new(Vec::new()),
            media_source: Mutex::new(None),
            media_info_dirty: AtomicBool::new(false),
            // Start "pending" so the pump populates the snapshot immediately.
            wake: Mutex::new(true),
            condvar: Condvar::new(),
            running: AtomicBool::new(true),
        });

        for name in OBSERVED_PROPERTIES {
            let cname = CString::new(*name).expect("observed property names never contain nul");
            unsafe {
                ffi::mpv_observe_property(handle.as_ptr(), 0, cname.as_ptr(), ffi::MPV_FORMAT_NONE);
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

    pub(crate) fn ab_loop_state(&self) -> AbLoopState {
        lock(&self.shared.snapshot).ab_loop
    }

    pub(crate) fn subtitle_delay(&self) -> f64 {
        lock(&self.shared.snapshot).subtitle_delay
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

    /// Drain the lifecycle events queued since the last call, oldest first.
    pub(crate) fn take_lifecycle_events(&self) -> Vec<MpvEvent> {
        std::mem::take(&mut *lock(&self.shared.events))
    }

    /// Record the local path backing the current media so `media-info` reports
    /// the same title/path the shell would have passed synchronously. `None`
    /// for streams (matching the shell's URL handling).
    ///
    /// The `FileLoaded` recompute usually runs before the shell gets a chance to
    /// call this, so it builds `media_info` with no source. Flag `media_info`
    /// dirty and wake the pump so it rebuilds the snapshot against the path we
    /// just recorded instead of waiting for an unrelated list change.
    pub(crate) fn set_media_source(&self, source: Option<PathBuf>) {
        *lock(&self.shared.media_source) = source;
        self.shared.media_info_dirty.store(true, Ordering::Release);
        *lock(&self.shared.wake) = true;
        self.shared.condvar.notify_one();
    }

    /// Stop the wakeup callback, wake the pump so it observes the shutdown flag,
    /// and join it. Must run before the handle is destroyed.
    pub(crate) fn shutdown(mut self) {
        unsafe {
            ffi::mpv_set_wakeup_callback(self.handle.as_ptr(), None, ptr::null_mut());
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.condvar.notify_all();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
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

        let (lifecycle, mut flags) = drain_events(shared);
        // A `set_media_source` between the `FileLoaded` recompute and now only
        // updates the stored path, so fold its pending flag in here to rebuild
        // `media_info` against it.
        if shared.media_info_dirty.swap(false, Ordering::AcqRel) {
            flags.media_info = true;
        }
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
            ffi::MPV_EVENT_FILE_LOADED => {
                lifecycle.push(MpvEvent::FileLoaded);
                flags.tracks = true;
                flags.chapters = true;
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
                lifecycle.push(MpvEvent::EndFile { reason });
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
                        b"audio-device-list" | b"audio-device" => flags.audio_devices = true,
                        _ => {}
                    }
                }
            }
            ffi::MPV_EVENT_COMMAND_REPLY if event.error < 0 => {
                eprintln!("[okp-mpv] async command failed with code {}", event.error);
            }
            _ => {}
        }
    }

    (lifecycle, flags)
}

/// Read the current values off the background thread and publish them. Scalars
/// are cheap and refreshed on every wakeup; the heavier lists are only re-read
/// when a change flagged them, so playback ticks stay lightweight.
fn recompute(shared: &Arc<PumpShared>, flags: RecomputeFlags) {
    let reader = shared.reader;

    let playback = reader.playback_state().unwrap_or_default();
    let ab_loop = reader.ab_loop_state().unwrap_or_default();
    let subtitle_delay = reader.subtitle_delay().unwrap_or(0.0);
    let subtitle_scale = reader.subtitle_scale().unwrap_or(1.0);
    let speed = reader.speed().unwrap_or(1.0);
    let secondary_subtitle_id = reader.secondary_subtitle_id().unwrap_or_default();

    let tracks = flags.tracks.then(|| reader.tracks().unwrap_or_default());
    let chapters = flags
        .chapters
        .then(|| reader.chapters().unwrap_or_default());
    let audio_devices = flags
        .audio_devices
        .then(|| reader.audio_devices().unwrap_or_default());
    let media_info = flags.media_info.then(|| {
        let source = lock(&shared.media_source).clone();
        reader.media_info(source.as_deref()).ok()
    });

    let mut snapshot = lock(&shared.snapshot);
    snapshot.playback = playback;
    snapshot.ab_loop = ab_loop;
    snapshot.subtitle_delay = subtitle_delay;
    snapshot.subtitle_scale = subtitle_scale;
    snapshot.speed = speed;
    snapshot.secondary_subtitle_id = secondary_subtitle_id;
    if let Some(tracks) = tracks {
        snapshot.tracks = tracks;
    }
    if let Some(chapters) = chapters {
        snapshot.chapters = chapters;
    }
    if let Some(audio_devices) = audio_devices {
        snapshot.audio_devices = audio_devices;
    }
    if let Some(media_info) = media_info {
        snapshot.media_info = media_info;
    }
}

/// Lock a mutex, recovering the guard if the pump thread poisoned it — a busy
/// core must never turn a pump panic into a cascade of UI-thread panics.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
