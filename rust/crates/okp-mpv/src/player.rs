use std::ffi::{CString, NulError};
use std::path::Path;
use std::ptr::{self, NonNull};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use libc::{c_char, c_int, c_void};
use thiserror::Error;

use crate::ffi;

#[derive(Debug, Default, Clone, Copy)]
pub struct PlaybackState {
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: Option<f64>,
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

    pub fn set_volume(&self, volume: f64) -> Result<(), MpvError> {
        self.set_double("volume", volume.clamp(0.0, 130.0))
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

#[cfg(unix)]
fn path_to_cstring(path: &Path) -> Result<CString, NulError> {
    CString::new(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_to_cstring(path: &Path) -> Result<CString, NulError> {
    CString::new(path.to_string_lossy().as_bytes())
}
