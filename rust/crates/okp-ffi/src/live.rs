//! Live libmpv session C ABI used by the first native macOS shell.

use std::ffi::c_char;
use std::ptr;

use okp_core::player::{
    CommandOutcome, CommandReply, CommandResult, EndReason, OpenRequest, PlaybackStatus,
    PlayerCommand, PlayerError, PlayerErrorKind, PlayerEvent, PropertyChange, SeekMode, TrackKind,
};
use okp_core::playlist::PlaylistItem;
use okp_mpv::{EndFileReason, Mpv, MpvEvent, PlaybackState, error_description};

use crate::session_machine::SessionMachine;
use crate::{
    OkpCommand, OkpCommandOutcome, OkpPlaybackStatus, OkpRejectReason, command_from_c,
    outcome_to_c, rejected,
};

/// Version of the opt-in live-session ABI.
#[unsafe(no_mangle)]
pub extern "C" fn okp_live_abi_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OkpLiveResult {
    Ok = 0,
    InvalidArgument = 1,
    Rejected = 2,
    EngineError = 3,
}

#[repr(C)]
pub struct OkpLiveCommandResult {
    pub outcome: OkpCommandOutcome,
    pub result: OkpLiveResult,
}

/// One projection of the portable player snapshot plus edge-triggered engine facts
/// observed during this poll. Optional scalar values use an explicit `*_known` bit so
/// zero remains a valid position or duration.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct OkpLiveSnapshot {
    pub status: OkpPlaybackStatus,
    pub time_pos: f64,
    pub time_pos_known: bool,
    pub duration: f64,
    pub duration_known: bool,
    pub loaded: bool,
    pub playback_restarted: bool,
    pub ended: bool,
    pub shutdown: bool,
    pub error: bool,
}

impl Default for OkpLiveSnapshot {
    fn default() -> Self {
        Self {
            status: OkpPlaybackStatus::Idle,
            time_pos: 0.0,
            time_pos_known: false,
            duration: 0.0,
            duration_known: false,
            loaded: false,
            playback_restarted: false,
            ended: false,
            shutdown: false,
            error: false,
        }
    }
}

/// Opaque live session. Its `PlayerMachine` owns lifecycle truth; `Mpv` is only the
/// command/event/render adapter.
pub struct OkpLiveSession {
    state: SessionMachine,
    engine: Mpv,
    last_error: String,
}

/// Create an initialized libmpv session. The OpenGL render context is deliberately
/// created later, while the caller's NSOpenGLContext is current.
///
/// On failure, returns null and copies a nul-terminated message into `error_buffer`
/// when it is non-null and `error_capacity` is nonzero.
///
/// # Safety
/// `error_buffer`, when non-null, must identify writable storage of at least
/// `error_capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_new(
    error_buffer: *mut c_char,
    error_capacity: usize,
) -> *mut OkpLiveSession {
    let options = [
        #[cfg(target_os = "macos")]
        ("ao".to_owned(), "coreaudio".to_owned()),
        ("input-default-bindings".to_owned(), "no".to_owned()),
        ("input-vo-keyboard".to_owned(), "no".to_owned()),
        // The pinned macOS runtime has no Lua, so it exposes no osc option.
        #[cfg(not(target_os = "macos"))]
        ("osc".to_owned(), "no".to_owned()),
    ];
    match Mpv::new_with_options("auto-safe", &options) {
        Ok(mut engine) => {
            engine.start_event_pump();
            Box::into_raw(Box::new(OkpLiveSession {
                state: SessionMachine::new(),
                engine,
                last_error: String::new(),
            }))
        }
        Err(error) => {
            unsafe { copy_text(&error.to_string(), error_buffer, error_capacity) };
            ptr::null_mut()
        }
    }
}

/// Destroy a live session. The caller should first make its NSOpenGLContext current and
/// call [`okp_live_session_destroy_render_context`].
///
/// # Safety
/// `session` must be null or a live pointer returned by [`okp_live_session_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_free(session: *mut OkpLiveSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Apply a portable command, forwarding it to libmpv only when the shared Rust core
/// accepts it.
///
/// # Safety
/// Both pointers must be null or valid for the duration of this call. An `Open`
/// command's source follows the requirements of [`crate::okp_player_machine_apply_command`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_apply_command(
    session: *mut OkpLiveSession,
    command: *const OkpCommand,
) -> OkpLiveCommandResult {
    let (Some(session), Some(command)) = (unsafe { session.as_mut() }, unsafe { command.as_ref() })
    else {
        return live_command_result(
            rejected(OkpRejectReason::InvalidArgument),
            OkpLiveResult::InvalidArgument,
        );
    };
    let Some(command) = (unsafe { command_from_c(command) }) else {
        return live_command_result(
            rejected(OkpRejectReason::InvalidArgument),
            OkpLiveResult::InvalidArgument,
        );
    };

    session.dispatch(command)
}

/// Open one local media file through the portable `Open` command.
///
/// # Safety
/// `session` must be null or live and `path` must be null or a valid nul-terminated
/// UTF-8 string for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_open_file(
    session: *mut OkpLiveSession,
    path: *const c_char,
) -> OkpLiveCommandResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return invalid_command_result();
    };
    if path.is_null() {
        return invalid_command_result();
    }
    let Ok(path) = (unsafe { std::ffi::CStr::from_ptr(path) }).to_str() else {
        return invalid_command_result();
    };
    session.dispatch(PlayerCommand::Open(OpenRequest::new(PlaylistItem::Local(
        path.into(),
    ))))
}

/// Explicitly pause or resume through the portable command core.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_set_paused(
    session: *mut OkpLiveSession,
    paused: bool,
) -> OkpLiveCommandResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return invalid_command_result();
    };
    session.dispatch(PlayerCommand::SetPaused(paused))
}

/// Toggle pause through the portable command core.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_toggle_pause(
    session: *mut OkpLiveSession,
) -> OkpLiveCommandResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return invalid_command_result();
    };
    session.dispatch(PlayerCommand::TogglePause)
}

/// Stop and unload the current media through the portable command core.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_close(
    session: *mut OkpLiveSession,
) -> OkpLiveCommandResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return invalid_command_result();
    };
    session.dispatch(PlayerCommand::Close)
}

/// Create libmpv's OpenGL render context. The caller's NSOpenGLContext must be current.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_create_render_context(
    session: *mut OkpLiveSession,
) -> OkpLiveResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return OkpLiveResult::InvalidArgument;
    };
    session.last_error.clear();
    match session.engine.create_render_context(None, false) {
        Ok(()) => OkpLiveResult::Ok,
        Err(error) => session.record_engine_error(error.to_string()),
    }
}

/// Render the newest libmpv frame into the caller's current framebuffer.
///
/// # Safety
/// `session` must be null or a live session pointer. The same NSOpenGLContext used to
/// create the render context must be current on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_render(
    session: *mut OkpLiveSession,
    width: i32,
    height: i32,
) -> OkpLiveResult {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return OkpLiveResult::InvalidArgument;
    };
    match session.engine.render(width, height) {
        Ok(()) => OkpLiveResult::Ok,
        Err(error) => session.record_engine_error(error.to_string()),
    }
}

/// Release the libmpv render context while its NSOpenGLContext is still current.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_destroy_render_context(session: *mut OkpLiveSession) {
    if let Some(session) = unsafe { session.as_mut() } {
        session.engine.destroy_render_context();
    }
}

/// Drain engine events, reconcile the portable core, and return one immutable UI
/// projection. Edge flags are true only for events drained by this call.
///
/// # Safety
/// `session` must be null or a live session pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_poll(session: *mut OkpLiveSession) -> OkpLiveSnapshot {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return OkpLiveSnapshot::default();
    };

    let playback = session.engine.observed_playback_state();
    let lifecycle = session.engine.take_lifecycle_events();
    let mut edges = PollEdges::default();
    for event in lifecycle {
        session.reconcile_event(event, playback, &mut edges);
    }
    session.reconcile_playback(playback);
    session.snapshot(edges)
}

/// Copy the last session error as UTF-8. Returns the required byte count including the
/// trailing nul; callers can first pass null/zero to size a buffer.
///
/// # Safety
/// `session` must be null or a live session pointer. `buffer`, when non-null, must
/// identify writable storage of at least `capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_live_session_last_error(
    session: *const OkpLiveSession,
    buffer: *mut c_char,
    capacity: usize,
) -> usize {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return 0;
    };
    unsafe { copy_text(&session.last_error, buffer, capacity) }
}

#[derive(Default)]
struct PollEdges {
    loaded: bool,
    playback_restarted: bool,
    ended: bool,
    shutdown: bool,
    error: bool,
}

impl OkpLiveSession {
    fn dispatch(&mut self, command: PlayerCommand) -> OkpLiveCommandResult {
        self.last_error.clear();
        let engine = &self.engine;
        let (outcome, engine_result) = self.state.dispatch(&command, |command, request_id| {
            forward_command(engine, command, request_id)
        });
        let result = match (&outcome, engine_result) {
            (_, Err(message)) => {
                self.last_error = message;
                OkpLiveResult::EngineError
            }
            (CommandOutcome::Rejected(_), Ok(())) => OkpLiveResult::Rejected,
            _ => OkpLiveResult::Ok,
        };
        live_command_result(outcome_to_c(outcome), result)
    }

    fn record_engine_error(&mut self, message: String) -> OkpLiveResult {
        self.last_error = message;
        OkpLiveResult::EngineError
    }

    fn reconcile_playback(&mut self, playback: PlaybackState) {
        if self.state.machine().status() == PlaybackStatus::Idle {
            return;
        }
        self.state
            .apply_event(PlayerEvent::Property(PropertyChange::TimePos(
                playback.time_pos,
            )));
        self.state
            .apply_event(PlayerEvent::Property(PropertyChange::Duration(
                playback.duration,
            )));
        self.state
            .apply_event(PlayerEvent::Property(PropertyChange::Paused(
                playback.paused,
            )));
        self.state
            .apply_event(PlayerEvent::Property(PropertyChange::Volume(
                playback.volume,
            )));
        self.state
            .apply_event(PlayerEvent::Property(PropertyChange::Speed(playback.speed)));
    }

    fn reconcile_event(&mut self, event: MpvEvent, playback: PlaybackState, edges: &mut PollEdges) {
        match event {
            MpvEvent::FileLoaded { .. } => {
                self.state.apply_event(PlayerEvent::Loaded {
                    duration: playback.duration,
                });
                edges.loaded = true;
            }
            MpvEvent::PlaybackRestart { .. } => edges.playback_restarted = true,
            MpvEvent::EndFile {
                reason,
                diagnostic_messages,
                path,
                ..
            } => {
                if !event_matches_source(
                    self.state.machine().snapshot().source.as_ref(),
                    path.as_deref(),
                ) {
                    return;
                }
                if let Some(message) = endfile_error(reason, &diagnostic_messages) {
                    self.last_error = message.clone();
                    self.state.apply_event(PlayerEvent::Error(PlayerError {
                        kind: PlayerErrorKind::LoadFailed,
                        message,
                    }));
                    edges.error = true;
                }
                self.state
                    .apply_event(PlayerEvent::Ended(core_end_reason(reason)));
                edges.ended = true;
            }
            MpvEvent::CommandReply { request_id, error } => {
                let result = if error < 0 {
                    let message = error_description(error);
                    self.last_error = message.clone();
                    edges.error = true;
                    CommandResult::Failed(message)
                } else {
                    CommandResult::Ok
                };
                self.state
                    .apply_event(PlayerEvent::Reply(CommandReply { request_id, result }));
            }
            MpvEvent::Shutdown => {
                self.state.apply_event(PlayerEvent::Ended(EndReason::Quit));
                edges.shutdown = true;
                edges.ended = true;
            }
            MpvEvent::DecoderWarning { .. } | MpvEvent::VideoReconfig { .. } => {}
        }
    }

    fn snapshot(&self, edges: PollEdges) -> OkpLiveSnapshot {
        let snapshot = self.state.machine().snapshot();
        OkpLiveSnapshot {
            status: snapshot.status.into(),
            time_pos: snapshot.time_pos.unwrap_or_default(),
            time_pos_known: snapshot.time_pos.is_some(),
            duration: snapshot.duration.unwrap_or_default(),
            duration_known: snapshot.duration.is_some(),
            loaded: edges.loaded,
            playback_restarted: edges.playback_restarted,
            ended: edges.ended,
            shutdown: edges.shutdown,
            error: edges.error,
        }
    }
}

fn forward_command(engine: &Mpv, command: &PlayerCommand, request_id: u64) -> Result<(), String> {
    let result = match command {
        PlayerCommand::Open(request) => {
            let loaded = match &request.source {
                PlaylistItem::Local(path) => {
                    engine.set_media_source(Some(path.clone()));
                    engine.load_file(path)
                }
                PlaylistItem::Url(url) => {
                    engine.set_media_source(None);
                    engine.load_url(url)
                }
            };
            loaded.and_then(|()| {
                if let Some(seconds) = request.resume_from {
                    engine.seek_absolute(seconds)?;
                }
                if let Some(id) = request.initial_audio {
                    engine.select_audio(Some(id))?;
                }
                if let Some(id) = request.initial_subtitle {
                    engine.select_subtitle(Some(id))?;
                }
                Ok(())
            })
        }
        PlayerCommand::Close => engine.command_async_with_userdata(&["stop"], request_id),
        PlayerCommand::Seek(request) => match request.mode {
            SeekMode::Absolute => engine.command_async_with_userdata(
                &["seek", &request.seconds.to_string(), "absolute+exact"],
                request_id,
            ),
            SeekMode::Relative => engine.command_async_with_userdata(
                &["seek", &request.seconds.to_string(), "relative+exact"],
                request_id,
            ),
        },
        PlayerCommand::SetPaused(paused) => engine.command_async_with_userdata(
            &["set", "pause", if *paused { "yes" } else { "no" }],
            request_id,
        ),
        PlayerCommand::TogglePause => {
            engine.command_async_with_userdata(&["cycle", "pause"], request_id)
        }
        PlayerCommand::SelectTrack { kind, id } => match kind {
            TrackKind::Audio => engine.select_audio(*id),
            TrackKind::Subtitle => engine.select_subtitle(*id),
        },
        PlayerCommand::SetSubtitleDelay(seconds) => engine.set_subtitle_delay(*seconds),
        PlayerCommand::SetSpeed(speed) => engine.set_speed(*speed),
        PlayerCommand::SetVolume(volume) => engine.set_volume(*volume),
        PlayerCommand::RequestScreenshot { .. } => {
            return Err(
                "the live C ABI requires the native shell to choose a screenshot path".to_owned(),
            );
        }
    };
    result.map_err(|error| error.to_string())
}

fn event_matches_source(current: Option<&PlaylistItem>, ended_path: Option<&str>) -> bool {
    let Some(current) = current else { return false };
    ended_path.is_none_or(|ended| match current {
        PlaylistItem::Local(path) => path.to_string_lossy() == ended,
        PlaylistItem::Url(url) => url == ended,
    })
}

fn endfile_error(reason: EndFileReason, messages: &[String]) -> Option<String> {
    match reason {
        EndFileReason::Error(code) | EndFileReason::Unknown(code) => {
            let mut message = error_description(code);
            if let Some(diagnostic) = messages.last() {
                message.push_str(": ");
                message.push_str(diagnostic.trim());
            }
            Some(message)
        }
        EndFileReason::Eof => okp_core::playback_failure::diagnose_mpv_eof(
            messages,
            okp_core::playback_failure::CodecEnvironment::System,
        )
        .map(|diagnostic| diagnostic.detail),
        _ => None,
    }
}

fn core_end_reason(reason: EndFileReason) -> EndReason {
    match reason {
        EndFileReason::Eof => EndReason::Eof,
        EndFileReason::Stop => EndReason::Stopped,
        EndFileReason::Quit => EndReason::Quit,
        EndFileReason::Error(_) | EndFileReason::Unknown(_) => EndReason::Error,
        EndFileReason::Redirect => EndReason::Redirect,
    }
}

fn live_command_result(outcome: OkpCommandOutcome, result: OkpLiveResult) -> OkpLiveCommandResult {
    OkpLiveCommandResult { outcome, result }
}

fn invalid_command_result() -> OkpLiveCommandResult {
    live_command_result(
        rejected(OkpRejectReason::InvalidArgument),
        OkpLiveResult::InvalidArgument,
    )
}

/// Copy bytes and always terminate when capacity allows. Interior nuls from foreign
/// diagnostic text are replaced so C consumers never observe a truncated message.
unsafe fn copy_text(text: &str, buffer: *mut c_char, capacity: usize) -> usize {
    let required = text.len().saturating_add(1);
    if buffer.is_null() || capacity == 0 {
        return required;
    }
    let count = text.len().min(capacity.saturating_sub(1));
    for (index, byte) in text.as_bytes()[..count].iter().copied().enumerate() {
        unsafe {
            buffer
                .add(index)
                .write(if byte == 0 { b'?' } else { byte } as c_char)
        };
    }
    unsafe { buffer.add(count).write(0) };
    required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superseded_source_errors_do_not_match_the_current_video() {
        let current = PlaylistItem::Local("/media/b.mp4".into());
        assert!(!event_matches_source(Some(&current), Some("/media/a.mp4")));
        assert!(event_matches_source(Some(&current), Some("/media/b.mp4")));
        assert!(event_matches_source(Some(&current), None));
        assert!(!event_matches_source(None, None));
    }

    #[test]
    fn decoder_failure_at_eof_is_reported_but_normal_eof_is_not() {
        let messages = vec!["Failed to open codec".to_owned()];
        assert!(endfile_error(EndFileReason::Eof, &messages).is_some());
        assert!(endfile_error(EndFileReason::Eof, &[]).is_none());
        assert!(endfile_error(EndFileReason::Stop, &messages).is_none());
    }

    #[test]
    fn error_copy_reports_required_size_and_nul_terminates_truncation() {
        let mut buffer = [b'x' as c_char; 5];
        let required = unsafe { copy_text("abcdef", buffer.as_mut_ptr(), buffer.len()) };

        assert_eq!(required, 7);
        assert_eq!(
            &buffer,
            &[
                b'a' as c_char,
                b'b' as c_char,
                b'c' as c_char,
                b'd' as c_char,
                0
            ]
        );
    }

    #[test]
    fn end_file_reasons_keep_core_meaning() {
        assert_eq!(core_end_reason(EndFileReason::Eof), EndReason::Eof);
        assert_eq!(core_end_reason(EndFileReason::Stop), EndReason::Stopped);
        assert_eq!(core_end_reason(EndFileReason::Error(-12)), EndReason::Error);
        assert_eq!(
            core_end_reason(EndFileReason::Redirect),
            EndReason::Redirect
        );
    }
}
