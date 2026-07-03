//! C-ABI scaffold projecting the `okp-core` player contract (core-extraction epic
//! C10, issue #152).
//!
//! It wraps the pure [`okp_core::player::PlayerMachine`] behind an opaque handle and
//! mirrors the core command/outcome/status enums as `#[repr(C)]` types a C consumer can
//! build and read. The C header (`okp_core.h`) is generated from these declarations by
//! cbindgen at build time — see `build.rs`. This is a scaffold: it exercises the whole
//! contract shape end to end (command in, engine notification in, state out) without yet
//! binding to a live engine, which arrives when `okp-mpv` becomes event-driven.

use std::ffi::{CStr, c_char};
use std::path::PathBuf;

use okp_core::player::{
    CommandOutcome, EndReason, OpenRequest, PlaybackStatus, PlayerCommand, PlayerEvent,
    PlayerMachine, RejectReason, SeekMode, SeekRequest, TrackKind,
};
use okp_core::playlist::PlaylistItem;

/// ABI version of the generated header. Bump on any breaking change to the exported
/// types or functions below.
#[unsafe(no_mangle)]
pub extern "C" fn okp_core_abi_version() -> u32 {
    1
}

/// Lifecycle state of the player. Mirrors [`PlaybackStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpPlaybackStatus {
    Idle = 0,
    Opening = 1,
    Playing = 2,
    Paused = 3,
    Ended = 4,
}

/// Whether a track is an audio or a subtitle stream. Mirrors [`TrackKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpTrackKind {
    Audio = 0,
    Subtitle = 1,
}

/// Whether a seek target is absolute or relative. Mirrors [`SeekMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpSeekMode {
    Absolute = 0,
    Relative = 1,
}

/// Why the current media stopped. Mirrors [`EndReason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpEndReason {
    Eof = 0,
    Stopped = 1,
    Quit = 2,
    Error = 3,
    Redirect = 4,
}

/// Which command an [`OkpCommand`] carries; selects which of its fields are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpCommandKind {
    Open = 0,
    Close = 1,
    Seek = 2,
    SetPaused = 3,
    TogglePause = 4,
    SelectTrack = 5,
    SetSubtitleDelay = 6,
    SetSpeed = 7,
    SetVolume = 8,
    RequestScreenshot = 9,
}

/// Discriminant of an [`OkpCommandOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpOutcomeKind {
    Accepted = 0,
    NoOp = 1,
    Rejected = 2,
}

/// Why the machine rejected a command, or `None` when it did not. Mirrors
/// [`RejectReason`] with an added `None` sentinel and an `InvalidArgument` value the
/// FFI boundary raises for null/undecodable input (the core never produces it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpRejectReason {
    None = 0,
    NoActiveMedia = 1,
    NotFinite = 2,
    InvalidArgument = 3,
}

/// A command a C consumer fills in and passes to [`okp_player_machine_apply_command`].
/// `kind` selects which of the remaining fields are meaningful; the rest are ignored.
#[repr(C)]
pub struct OkpCommand {
    /// Which command this is.
    pub kind: OkpCommandKind,
    /// `Open` source: a UTF-8, nul-terminated path or URL. Null for other kinds.
    pub source: *const c_char,
    /// `Open`: whether `source` is a stream URL (`true`) or a local path (`false`).
    pub source_is_url: bool,
    /// `Open`: resume position in seconds; a negative value means "no resume".
    pub resume_from: f64,
    /// `Open`: audio track id to preselect; a negative value means "none".
    pub initial_audio: i64,
    /// `Open`: subtitle track id to preselect; a negative value means "none".
    pub initial_subtitle: i64,
    /// `Seek` / `SetSubtitleDelay` / `SetSpeed` / `SetVolume`: the scalar argument.
    pub value: f64,
    /// `Seek`: whether `value` is absolute or relative.
    pub seek_mode: OkpSeekMode,
    /// `SetPaused`: the paused flag. `RequestScreenshot`: include subtitles.
    pub flag: bool,
    /// `SelectTrack`: which track list.
    pub track_kind: OkpTrackKind,
    /// `SelectTrack`: the track id; a negative value means "deselect / off".
    pub track_id: i64,
}

/// The result of [`okp_player_machine_apply_command`], returned by value.
#[repr(C)]
pub struct OkpCommandOutcome {
    pub kind: OkpOutcomeKind,
    /// Set when `kind == Accepted`; correlates the eventual reply. 0 otherwise.
    pub request_id: u64,
    /// Set when `kind == Rejected`; `None` otherwise.
    pub reject_reason: OkpRejectReason,
}

/// Opaque handle to a player state machine. Create with [`okp_player_machine_new`] and
/// release with [`okp_player_machine_free`].
pub struct OkpPlayerMachine {
    inner: PlayerMachine,
}

/// Create a new player state machine (starts in the `Idle` state). Never returns null;
/// release the returned handle with [`okp_player_machine_free`].
#[unsafe(no_mangle)]
pub extern "C" fn okp_player_machine_new() -> *mut OkpPlayerMachine {
    Box::into_raw(Box::new(OkpPlayerMachine {
        inner: PlayerMachine::new(),
    }))
}

/// Release a handle returned by [`okp_player_machine_new`]. A null pointer is ignored.
///
/// # Safety
/// `machine` must be a pointer returned by [`okp_player_machine_new`] that has not
/// already been freed. The pointer is dangling after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_player_machine_free(machine: *mut OkpPlayerMachine) {
    if !machine.is_null() {
        drop(unsafe { Box::from_raw(machine) });
    }
}

/// The current lifecycle status. Returns `Idle` for a null handle.
///
/// # Safety
/// `machine` must be null or a live handle from [`okp_player_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_player_machine_status(
    machine: *const OkpPlayerMachine,
) -> OkpPlaybackStatus {
    match unsafe { machine.as_ref() } {
        Some(machine) => machine.inner.status().into(),
        None => OkpPlaybackStatus::Idle,
    }
}

/// Feed a command to the machine, returning the outcome by value. A null handle, a null
/// command, or an `Open` whose `source` is not valid UTF-8 yields a `Rejected` outcome
/// with `InvalidArgument` and changes nothing.
///
/// # Safety
/// `machine` must be null or a live handle from [`okp_player_machine_new`]. `command`
/// must be null or point to a valid [`OkpCommand`]; when its `kind` is `Open`, `source`
/// must be null or a valid nul-terminated string for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_player_machine_apply_command(
    machine: *mut OkpPlayerMachine,
    command: *const OkpCommand,
) -> OkpCommandOutcome {
    let (Some(machine), Some(command)) = (unsafe { machine.as_mut() }, unsafe { command.as_ref() })
    else {
        return rejected(OkpRejectReason::InvalidArgument);
    };
    match unsafe { command_from_c(command) } {
        Some(command) => outcome_to_c(machine.inner.apply_command(&command)),
        None => rejected(OkpRejectReason::InvalidArgument),
    }
}

/// Notify the machine that the current media finished loading. `duration` is in seconds;
/// a negative value means "unknown".
///
/// # Safety
/// `machine` must be null or a live handle from [`okp_player_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_player_machine_notify_loaded(
    machine: *mut OkpPlayerMachine,
    duration: f64,
) {
    if let Some(machine) = unsafe { machine.as_mut() } {
        machine.inner.apply_event(PlayerEvent::Loaded {
            duration: non_negative(duration),
        });
    }
}

/// Notify the machine that playback of the current media ended.
///
/// # Safety
/// `machine` must be null or a live handle from [`okp_player_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_player_machine_notify_ended(
    machine: *mut OkpPlayerMachine,
    reason: OkpEndReason,
) {
    if let Some(machine) = unsafe { machine.as_mut() } {
        machine.inner.apply_event(PlayerEvent::Ended(reason.into()));
    }
}

fn rejected(reason: OkpRejectReason) -> OkpCommandOutcome {
    OkpCommandOutcome {
        kind: OkpOutcomeKind::Rejected,
        request_id: 0,
        reject_reason: reason,
    }
}

fn outcome_to_c(outcome: CommandOutcome) -> OkpCommandOutcome {
    match outcome {
        CommandOutcome::Accepted { request_id } => OkpCommandOutcome {
            kind: OkpOutcomeKind::Accepted,
            request_id,
            reject_reason: OkpRejectReason::None,
        },
        CommandOutcome::NoOp => OkpCommandOutcome {
            kind: OkpOutcomeKind::NoOp,
            request_id: 0,
            reject_reason: OkpRejectReason::None,
        },
        CommandOutcome::Rejected(reason) => rejected(reason.into()),
    }
}

/// Build a core [`PlayerCommand`] from its C descriptor. Returns `None` when an `Open`
/// carries a null or non-UTF-8 `source`.
///
/// # Safety
/// When `command.kind` is `Open`, `command.source` must be null or a valid
/// nul-terminated string.
unsafe fn command_from_c(command: &OkpCommand) -> Option<PlayerCommand> {
    let parsed = match command.kind {
        OkpCommandKind::Open => PlayerCommand::Open(OpenRequest {
            source: unsafe { decode_source(command.source, command.source_is_url) }?,
            resume_from: non_negative(command.resume_from),
            initial_audio: non_negative_id(command.initial_audio),
            initial_subtitle: non_negative_id(command.initial_subtitle),
        }),
        OkpCommandKind::Close => PlayerCommand::Close,
        OkpCommandKind::Seek => PlayerCommand::Seek(SeekRequest {
            mode: command.seek_mode.into(),
            seconds: command.value,
        }),
        OkpCommandKind::SetPaused => PlayerCommand::SetPaused(command.flag),
        OkpCommandKind::TogglePause => PlayerCommand::TogglePause,
        OkpCommandKind::SelectTrack => PlayerCommand::SelectTrack {
            kind: command.track_kind.into(),
            id: non_negative_id(command.track_id),
        },
        OkpCommandKind::SetSubtitleDelay => PlayerCommand::SetSubtitleDelay(command.value),
        OkpCommandKind::SetSpeed => PlayerCommand::SetSpeed(command.value),
        OkpCommandKind::SetVolume => PlayerCommand::SetVolume(command.value),
        OkpCommandKind::RequestScreenshot => PlayerCommand::RequestScreenshot {
            include_subtitles: command.flag,
        },
    };
    Some(parsed)
}

/// # Safety
/// `source` must be null or a valid nul-terminated string.
unsafe fn decode_source(source: *const c_char, is_url: bool) -> Option<PlaylistItem> {
    if source.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(source) }.to_str().ok()?.to_owned();
    Some(if is_url {
        PlaylistItem::Url(text)
    } else {
        PlaylistItem::Local(PathBuf::from(text))
    })
}

fn non_negative(value: f64) -> Option<f64> {
    (value >= 0.0).then_some(value)
}

fn non_negative_id(id: i64) -> Option<i64> {
    (id >= 0).then_some(id)
}

impl From<PlaybackStatus> for OkpPlaybackStatus {
    fn from(status: PlaybackStatus) -> Self {
        match status {
            PlaybackStatus::Idle => Self::Idle,
            PlaybackStatus::Opening => Self::Opening,
            PlaybackStatus::Playing => Self::Playing,
            PlaybackStatus::Paused => Self::Paused,
            PlaybackStatus::Ended => Self::Ended,
        }
    }
}

impl From<OkpSeekMode> for SeekMode {
    fn from(mode: OkpSeekMode) -> Self {
        match mode {
            OkpSeekMode::Absolute => Self::Absolute,
            OkpSeekMode::Relative => Self::Relative,
        }
    }
}

impl From<OkpTrackKind> for TrackKind {
    fn from(kind: OkpTrackKind) -> Self {
        match kind {
            OkpTrackKind::Audio => Self::Audio,
            OkpTrackKind::Subtitle => Self::Subtitle,
        }
    }
}

impl From<OkpEndReason> for EndReason {
    fn from(reason: OkpEndReason) -> Self {
        match reason {
            OkpEndReason::Eof => Self::Eof,
            OkpEndReason::Stopped => Self::Stopped,
            OkpEndReason::Quit => Self::Quit,
            OkpEndReason::Error => Self::Error,
            OkpEndReason::Redirect => Self::Redirect,
        }
    }
}

impl From<RejectReason> for OkpRejectReason {
    fn from(reason: RejectReason) -> Self {
        match reason {
            RejectReason::NoActiveMedia => Self::NoActiveMedia,
            RejectReason::NotFinite => Self::NotFinite,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    /// A zeroed-out command of the given kind (all optional fields set to "none").
    fn command(kind: OkpCommandKind) -> OkpCommand {
        OkpCommand {
            kind,
            source: ptr::null(),
            source_is_url: false,
            resume_from: -1.0,
            initial_audio: -1,
            initial_subtitle: -1,
            value: 0.0,
            seek_mode: OkpSeekMode::Absolute,
            flag: false,
            track_kind: OkpTrackKind::Audio,
            track_id: -1,
        }
    }

    #[test]
    fn abi_version_starts_at_one() {
        assert_eq!(okp_core_abi_version(), 1);
    }

    #[test]
    fn machine_round_trips_open_load_and_end() {
        unsafe {
            let machine = okp_player_machine_new();
            assert_eq!(okp_player_machine_status(machine), OkpPlaybackStatus::Idle);

            let source = CString::new("/media/a.mkv").unwrap();
            let mut open = command(OkpCommandKind::Open);
            open.source = source.as_ptr();
            let outcome = okp_player_machine_apply_command(machine, &open);
            assert_eq!(outcome.kind, OkpOutcomeKind::Accepted);
            assert_eq!(outcome.request_id, 1);
            assert_eq!(
                okp_player_machine_status(machine),
                OkpPlaybackStatus::Opening
            );

            okp_player_machine_notify_loaded(machine, 120.0);
            assert_eq!(
                okp_player_machine_status(machine),
                OkpPlaybackStatus::Playing
            );

            let mut pause = command(OkpCommandKind::SetPaused);
            pause.flag = true;
            let outcome = okp_player_machine_apply_command(machine, &pause);
            assert_eq!(outcome.kind, OkpOutcomeKind::Accepted);
            assert_eq!(outcome.request_id, 2);
            assert_eq!(
                okp_player_machine_status(machine),
                OkpPlaybackStatus::Paused
            );

            okp_player_machine_notify_ended(machine, OkpEndReason::Eof);
            assert_eq!(okp_player_machine_status(machine), OkpPlaybackStatus::Ended);

            okp_player_machine_free(machine);
        }
    }

    #[test]
    fn close_while_idle_is_a_noop() {
        unsafe {
            let machine = okp_player_machine_new();
            let outcome =
                okp_player_machine_apply_command(machine, &command(OkpCommandKind::Close));
            assert_eq!(outcome.kind, OkpOutcomeKind::NoOp);
            assert_eq!(outcome.request_id, 0);
            okp_player_machine_free(machine);
        }
    }

    #[test]
    fn non_finite_resume_open_is_rejected() {
        unsafe {
            let machine = okp_player_machine_new();
            let source = CString::new("/media/a.mkv").unwrap();
            let mut open = command(OkpCommandKind::Open);
            open.source = source.as_ptr();
            // An infinite resume clears `non_negative`'s sentinel gate, so it reaches the
            // core as a real value and must be refused rather than seeded into the state.
            open.resume_from = f64::INFINITY;
            let outcome = okp_player_machine_apply_command(machine, &open);
            assert_eq!(outcome.kind, OkpOutcomeKind::Rejected);
            assert_eq!(outcome.reject_reason, OkpRejectReason::NotFinite);
            assert_eq!(okp_player_machine_status(machine), OkpPlaybackStatus::Idle);
            okp_player_machine_free(machine);
        }
    }

    #[test]
    fn non_finite_seek_is_rejected() {
        unsafe {
            let machine = okp_player_machine_new();
            let mut seek = command(OkpCommandKind::Seek);
            seek.value = f64::NAN;
            let outcome = okp_player_machine_apply_command(machine, &seek);
            assert_eq!(outcome.kind, OkpOutcomeKind::Rejected);
            assert_eq!(outcome.reject_reason, OkpRejectReason::NotFinite);
            okp_player_machine_free(machine);
        }
    }

    #[test]
    fn null_handle_reports_idle_and_rejects_commands() {
        unsafe {
            assert_eq!(
                okp_player_machine_status(ptr::null()),
                OkpPlaybackStatus::Idle
            );
            let outcome =
                okp_player_machine_apply_command(ptr::null_mut(), &command(OkpCommandKind::Close));
            assert_eq!(outcome.kind, OkpOutcomeKind::Rejected);
            assert_eq!(outcome.reject_reason, OkpRejectReason::InvalidArgument);
        }
    }

    #[test]
    fn open_with_null_source_is_rejected() {
        unsafe {
            let machine = okp_player_machine_new();
            let outcome = okp_player_machine_apply_command(machine, &command(OkpCommandKind::Open));
            assert_eq!(outcome.kind, OkpOutcomeKind::Rejected);
            assert_eq!(outcome.reject_reason, OkpRejectReason::InvalidArgument);
            okp_player_machine_free(machine);
        }
    }
}
