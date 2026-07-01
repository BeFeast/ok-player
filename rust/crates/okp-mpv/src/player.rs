use std::ffi::{CStr, CString, NulError};
use std::path::Path;
use std::ptr::{self, NonNull};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use libc::{c_char, c_int, c_void};
use thiserror::Error;

use crate::ffi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PlaybackState {
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub selected: bool,
    pub external: bool,
    pub default: bool,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub audio_channels: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub index: i64,
    pub time: f64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvEvent {
    EndFile { reason: EndFileReason },
    FileLoaded,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndFileReason {
    Eof,
    Stop,
    Quit,
    Error(c_int),
    Redirect,
    Unknown(c_int),
}

impl EndFileReason {
    pub fn is_eof(self) -> bool {
        matches!(self, Self::Eof)
    }
}

#[derive(Debug, Error)]
pub enum MpvError {
    #[error("libmpv returned null handle")]
    NullHandle,
    #[error("string contains an interior nul byte")]
    InteriorNul(#[from] NulError),
    #[error("libmpv call failed with code {0}")]
    LibMpv(c_int),
    #[error("libmpv render context is not initialized")]
    MissingRenderContext,
}

pub struct Mpv {
    handle: NonNull<ffi::mpv_handle>,
    render_context: Option<NonNull<ffi::mpv_render_context>>,
}

impl Mpv {
    pub fn new() -> Result<Self, MpvError> {
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }

        let handle = NonNull::new(unsafe { ffi::mpv_create() }).ok_or(MpvError::NullHandle)?;
        let this = Self {
            handle,
            render_context: None,
        };

        this.set_option("terminal", "no")?;
        this.set_option("config", "no")?;
        this.set_option("idle", "yes")?;
        this.set_option("force-window", "no")?;
        this.set_option("vo", "libmpv")?;
        this.set_option("hwdec", "no")?;
        check(unsafe { ffi::mpv_initialize(this.handle.as_ptr()) })?;

        Ok(this)
    }

    pub fn create_render_context(&mut self) -> Result<(), MpvError> {
        if self.render_context.is_some() {
            return Ok(());
        }

        let api = CString::new("opengl")?;
        let mut init_params = ffi::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = [
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_API_TYPE,
                data: api.as_ptr() as *mut c_void,
            },
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let mut context = ptr::null_mut();
        check(unsafe {
            ffi::mpv_render_context_create(&mut context, self.handle.as_ptr(), params.as_mut_ptr())
        })?;
        self.render_context = NonNull::new(context);

        Ok(())
    }

    pub fn load_file(&self, path: &Path) -> Result<(), MpvError> {
        let command = CString::new("loadfile")?;
        let path = path_to_cstring(path)?;
        let args = [command.as_ptr(), path.as_ptr(), ptr::null()];

        check(unsafe { ffi::mpv_command(self.handle.as_ptr(), args.as_ptr()) })
    }

    pub fn add_subtitle_file(&self, path: &Path) -> Result<(), MpvError> {
        let command = CString::new("sub-add")?;
        let path = path_to_cstring(path)?;
        let select = CString::new("select")?;
        let args = [
            command.as_ptr(),
            path.as_ptr(),
            select.as_ptr(),
            ptr::null(),
        ];

        check(unsafe { ffi::mpv_command(self.handle.as_ptr(), args.as_ptr()) })
    }

    pub fn tracks(&self) -> Result<Vec<Track>, MpvError> {
        let count = self.get_i64("track-list/count")?.unwrap_or(0).max(0);
        let mut tracks = Vec::new();

        for index in 0..count {
            let prefix = format!("track-list/{index}");
            let Some(kind) = self.get_string(&format!("{prefix}/type"))? else {
                continue;
            };
            let kind = match kind.as_str() {
                "audio" => TrackKind::Audio,
                "sub" => TrackKind::Subtitle,
                _ => continue,
            };

            tracks.push(Track {
                id: self.get_i64(&format!("{prefix}/id"))?.unwrap_or(0),
                kind,
                selected: self
                    .get_flag(&format!("{prefix}/selected"))?
                    .unwrap_or(false),
                external: self
                    .get_flag(&format!("{prefix}/external"))?
                    .unwrap_or(false),
                default: self
                    .get_flag(&format!("{prefix}/default"))?
                    .unwrap_or(false),
                title: self.get_string(&format!("{prefix}/title"))?,
                lang: self.get_string(&format!("{prefix}/lang"))?,
                codec: self.get_string(&format!("{prefix}/codec"))?,
                audio_channels: self.get_string(&format!("{prefix}/audio-channels"))?,
            });
        }

        Ok(tracks)
    }

    pub fn chapters(&self) -> Result<Vec<Chapter>, MpvError> {
        let count = self.get_i64("chapter-list/count")?.unwrap_or(0).max(0);
        let mut chapters = Vec::new();

        for index in 0..count {
            let prefix = format!("chapter-list/{index}");
            let Some(time) = self.get_double(&format!("{prefix}/time"))? else {
                continue;
            };

            chapters.push(Chapter {
                index,
                time,
                title: self.get_string(&format!("{prefix}/title"))?,
            });
        }

        Ok(chapters)
    }

    pub fn playback_state(&self) -> Result<PlaybackState, MpvError> {
        Ok(PlaybackState {
            time_pos: self.get_double("time-pos")?,
            duration: self.get_double("duration")?,
            paused: self.get_flag("pause")?.unwrap_or(false),
            volume: self.get_double("volume")?,
        })
    }

    pub fn cycle_pause(&self) -> Result<(), MpvError> {
        self.command(&["cycle", "pause"])
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), MpvError> {
        let seconds = seconds.max(0.0).to_string();
        self.command(&["seek", &seconds, "absolute+exact"])
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), MpvError> {
        self.command(&["seek", &seconds.to_string(), "relative+exact"])
    }

    pub fn frame_step(&self) -> Result<(), MpvError> {
        self.command(&["frame-step"])
    }

    pub fn frame_back_step(&self) -> Result<(), MpvError> {
        self.command(&["frame-back-step"])
    }

    pub fn screenshot_to_file(&self, path: &Path, include_subtitles: bool) -> Result<(), MpvError> {
        let path = path.to_string_lossy();
        let mode = if include_subtitles {
            "subtitles"
        } else {
            "video"
        };
        self.command(&["screenshot-to-file", &path, mode])
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), MpvError> {
        self.set_double("volume", volume.clamp(0.0, 130.0))
    }

    pub fn select_subtitle(&self, id: Option<i64>) -> Result<(), MpvError> {
        let value = track_id_or_off(id);
        self.command(&["set", "sid", &value])
    }

    pub fn select_secondary_subtitle(&self, id: Option<i64>) -> Result<(), MpvError> {
        let value = track_id_or_off(id);
        self.command(&["set", "secondary-sid", &value])
    }

    pub fn secondary_subtitle_id(&self) -> Result<Option<i64>, MpvError> {
        Ok(self.get_i64("secondary-sid")?.filter(|id| *id > 0))
    }

    pub fn select_audio(&self, id: Option<i64>) -> Result<(), MpvError> {
        let value = track_id_or_off(id);
        self.command(&["set", "aid", &value])
    }

    pub fn subtitle_delay(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("sub-delay")?.unwrap_or(0.0))
    }

    pub fn set_subtitle_delay(&self, seconds: f64) -> Result<(), MpvError> {
        self.set_double("sub-delay", seconds.clamp(-600.0, 600.0))
    }

    pub fn adjust_subtitle_delay(&self, delta_seconds: f64) -> Result<(), MpvError> {
        let delay = self.subtitle_delay()?;
        self.set_subtitle_delay(delay + delta_seconds)
    }

    pub fn subtitle_scale(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("sub-scale")?.unwrap_or(1.0))
    }

    pub fn set_subtitle_scale(&self, scale: f64) -> Result<(), MpvError> {
        self.set_double("sub-scale", scale.clamp(0.25, 4.0))
    }

    pub fn adjust_subtitle_scale(&self, delta: f64) -> Result<(), MpvError> {
        let scale = self.subtitle_scale()?;
        self.set_subtitle_scale(scale + delta)
    }

    pub fn drain_events(&self) -> Vec<MpvEvent> {
        let mut events = Vec::new();

        loop {
            let event = unsafe { ffi::mpv_wait_event(self.handle.as_ptr(), 0.0) };
            let Some(event) = (unsafe { event.as_ref() }) else {
                break;
            };

            match event.event_id {
                ffi::MPV_EVENT_NONE => break,
                ffi::MPV_EVENT_SHUTDOWN => events.push(MpvEvent::Shutdown),
                ffi::MPV_EVENT_FILE_LOADED => events.push(MpvEvent::FileLoaded),
                ffi::MPV_EVENT_END_FILE => {
                    let reason = if let Some(end_file) =
                        unsafe { event.data.cast::<ffi::mpv_event_end_file>().as_ref() }
                    {
                        end_file_reason(end_file.reason, end_file.error)
                    } else {
                        EndFileReason::Unknown(event.error)
                    };
                    events.push(MpvEvent::EndFile { reason });
                }
                _ => {}
            }
        }

        events
    }

    pub fn render(&mut self, width: i32, height: i32) -> Result<(), MpvError> {
        if width <= 0 || height <= 0 {
            return Ok(());
        }

        let context = self
            .render_context
            .ok_or(MpvError::MissingRenderContext)?
            .as_ptr();
        unsafe {
            let _ = ffi::mpv_render_context_update(context);
        }

        let mut framebuffer: c_int = 0;
        unsafe {
            ffi::glGetIntegerv(ffi::GL_FRAMEBUFFER_BINDING, &mut framebuffer);
            ffi::glViewport(0, 0, width, height);
        }

        let mut fbo = ffi::mpv_opengl_fbo {
            fbo: framebuffer,
            w: width,
            h: height,
            internal_format: 0,
        };
        let mut flip_y: c_int = 1;
        let mut params = [
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo as *mut _ as *mut c_void,
            },
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y as *mut _ as *mut c_void,
            },
            ffi::mpv_render_param {
                param_type: ffi::MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        check(unsafe { ffi::mpv_render_context_render(context, params.as_mut_ptr()) })?;
        unsafe {
            ffi::mpv_render_context_report_swap(context);
        }

        Ok(())
    }

    pub fn destroy_render_context(&mut self) {
        if let Some(context) = self.render_context.take() {
            unsafe {
                ffi::mpv_render_context_free(context.as_ptr());
            }
        }
    }

    fn set_option(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;

        check(unsafe {
            ffi::mpv_set_option_string(self.handle.as_ptr(), name.as_ptr(), value.as_ptr())
        })
    }

    fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        let c_args = args
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ptrs = c_args.iter().map(|arg| arg.as_ptr()).collect::<Vec<_>>();
        ptrs.push(ptr::null());

        check(unsafe { ffi::mpv_command(self.handle.as_ptr(), ptrs.as_ptr()) })
    }

    fn get_double(&self, name: &str) -> Result<Option<f64>, MpvError> {
        let name = CString::new(name)?;
        let mut value = 0.0;
        let code = unsafe {
            ffi::mpv_get_property(
                self.handle.as_ptr(),
                name.as_ptr(),
                ffi::MPV_FORMAT_DOUBLE,
                &mut value as *mut _ as *mut c_void,
            )
        };

        if code < 0 { Ok(None) } else { Ok(Some(value)) }
    }

    fn get_flag(&self, name: &str) -> Result<Option<bool>, MpvError> {
        let name = CString::new(name)?;
        let mut value: c_int = 0;
        let code = unsafe {
            ffi::mpv_get_property(
                self.handle.as_ptr(),
                name.as_ptr(),
                ffi::MPV_FORMAT_FLAG,
                &mut value as *mut _ as *mut c_void,
            )
        };

        if code < 0 {
            Ok(None)
        } else {
            Ok(Some(value != 0))
        }
    }

    fn get_i64(&self, name: &str) -> Result<Option<i64>, MpvError> {
        let name = CString::new(name)?;
        let mut value: i64 = 0;
        let code = unsafe {
            ffi::mpv_get_property(
                self.handle.as_ptr(),
                name.as_ptr(),
                ffi::MPV_FORMAT_INT64,
                &mut value as *mut _ as *mut c_void,
            )
        };

        if code < 0 { Ok(None) } else { Ok(Some(value)) }
    }

    fn get_string(&self, name: &str) -> Result<Option<String>, MpvError> {
        let name = CString::new(name)?;
        let value = unsafe { ffi::mpv_get_property_string(self.handle.as_ptr(), name.as_ptr()) };
        if value.is_null() {
            return Ok(None);
        }

        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            ffi::mpv_free(value.cast::<c_void>());
        }

        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }

    fn set_double(&self, name: &str, mut value: f64) -> Result<(), MpvError> {
        let name = CString::new(name)?;

        check(unsafe {
            ffi::mpv_set_property(
                self.handle.as_ptr(),
                name.as_ptr(),
                ffi::MPV_FORMAT_DOUBLE,
                &mut value as *mut _ as *mut c_void,
            )
        })
    }
}

pub fn current_render_target_size() -> Option<RenderTargetSize> {
    let mut viewport: [c_int; 4] = [0; 4];
    unsafe {
        ffi::glGetIntegerv(ffi::GL_VIEWPORT, viewport.as_mut_ptr());
    }

    let width = viewport[2];
    let height = viewport[3];
    if width > 0 && height > 0 {
        Some(RenderTargetSize { width, height })
    } else {
        None
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.destroy_render_context();
        unsafe {
            ffi::mpv_terminate_destroy(self.handle.as_ptr());
        }
    }
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let glx = unsafe { ffi::glXGetProcAddressARB(name.cast::<u8>()) };
    if !glx.is_null() {
        return glx;
    }

    unsafe { ffi::eglGetProcAddress(name) }
}

fn check(code: c_int) -> Result<(), MpvError> {
    if code < 0 {
        Err(MpvError::LibMpv(code))
    } else {
        Ok(())
    }
}

fn end_file_reason(reason: c_int, error: c_int) -> EndFileReason {
    match reason {
        ffi::MPV_END_FILE_REASON_EOF => EndFileReason::Eof,
        ffi::MPV_END_FILE_REASON_STOP => EndFileReason::Stop,
        ffi::MPV_END_FILE_REASON_QUIT => EndFileReason::Quit,
        ffi::MPV_END_FILE_REASON_ERROR => EndFileReason::Error(error),
        ffi::MPV_END_FILE_REASON_REDIRECT => EndFileReason::Redirect,
        _ => EndFileReason::Unknown(reason),
    }
}

fn track_id_or_off(id: Option<i64>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "no".to_owned())
}

#[cfg(unix)]
fn path_to_cstring(path: &Path) -> Result<CString, NulError> {
    CString::new(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_to_cstring(path: &Path) -> Result<CString, NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}
