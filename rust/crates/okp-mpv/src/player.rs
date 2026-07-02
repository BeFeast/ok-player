use std::ffi::{CStr, CString, NulError};
use std::path::Path;
use std::ptr::{self, NonNull};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use libc::{c_char, c_int, c_void};
use thiserror::Error;

use crate::ffi;

const AUDIO_NORMALIZATION_FILTER_LABEL: &str = "@okpnorm";
const AUDIO_NORMALIZATION_FILTER: &str = "@okpnorm:dynaudnorm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetSize {
    pub width: i32,
    pub height: i32,
}

impl RenderTargetSize {
    fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }

    fn area(self) -> i64 {
        i64::from(self.width) * i64::from(self.height)
    }

    fn max_components(self, other: RenderTargetSize) -> RenderTargetSize {
        RenderTargetSize {
            width: self.width.max(other.width),
            height: self.height.max(other.height),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PlaybackState {
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: Option<f64>,
    pub speed: Option<f64>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    pub title: String,
    pub path: Option<String>,
    pub sections: Vec<InfoSection>,
    pub tracks: Vec<InfoTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoSection {
    pub title: String,
    pub rows: Vec<InfoRow>,
}

impl InfoSection {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            rows: Vec::new(),
        }
    }

    fn add(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        if !value.trim().is_empty() {
            self.rows.push(InfoRow {
                label: label.to_owned(),
                value,
            });
        }
    }

    fn add_option(&mut self, label: &str, value: Option<String>) {
        if let Some(value) = value {
            self.add(label, value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoRow {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoTrack {
    pub id: i64,
    pub kind: TrackKind,
    pub selected: bool,
    pub external: bool,
    pub default: bool,
    pub title: String,
    pub detail: String,
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
        Self::new_with_hwdec("no")
    }

    pub fn new_with_hwdec(hwdec: &str) -> Result<Self, MpvError> {
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
        this.set_option("hwdec", hwdec)?;
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

    pub fn load_url(&self, url: &str) -> Result<(), MpvError> {
        let command = CString::new("loadfile")?;
        let url = CString::new(url)?;
        let args = [command.as_ptr(), url.as_ptr(), ptr::null()];

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

    pub fn set_hwdec(&self, value: &str) -> Result<(), MpvError> {
        self.set_option("hwdec", value)
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
            speed: self.get_double("speed")?,
        })
    }

    pub fn media_info(&self, path: Option<&Path>) -> Result<MediaInfo, MpvError> {
        let title = path
            .map(display_path_name)
            .or_else(|| self.get_string("media-title").ok().flatten())
            .unwrap_or_else(|| "Untitled media".to_owned());
        let path_text = path.map(|path| path.display().to_string());
        let mut sections = Vec::new();

        let mut file = InfoSection::new("File");
        file.add_option(
            "Container",
            self.get_string("file-format")?
                .map(|container| friendly_container(&container)),
        );
        file.add_option(
            "Size",
            self.get_i64("file-size")?
                .filter(|size| *size >= 0)
                .map(format_bytes),
        );
        file.add_option(
            "Duration",
            self.get_double("duration")?
                .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                .map(format_duration),
        );
        file.add_option("Path", path_text.clone());
        push_section(&mut sections, file);

        let mut video = InfoSection::new("Video");
        video.add_option(
            "Codec",
            self.get_string("video-codec")?
                .map(|codec| friendly_codec(&codec)),
        );
        let width = self
            .get_i64("video-params/w")?
            .or(self.get_i64("width")?)
            .filter(|value| *value > 0);
        let height = self
            .get_i64("video-params/h")?
            .or(self.get_i64("height")?)
            .filter(|value| *value > 0);
        if let (Some(width), Some(height)) = (width, height) {
            video.add("Resolution", format!("{width} x {height}"));
        }
        if let Some(prefix) = self.selected_track_prefix("video")? {
            video.add_option(
                "Profile",
                self.get_string(&format!("{prefix}/codec-profile"))?,
            );
            video.add_option(
                "Bitrate",
                self.get_i64(&format!("{prefix}/demux-bitrate"))?
                    .filter(|bitrate| *bitrate > 0)
                    .map(format_bitrate),
            );
        }
        let display_width = self.get_i64("video-params/dw")?.filter(|value| *value > 0);
        let display_height = self.get_i64("video-params/dh")?.filter(|value| *value > 0);
        if let (Some(display_width), Some(display_height)) = (display_width, display_height)
            && (Some(display_width) != width || Some(display_height) != height)
        {
            video.add(
                "Display Size",
                format!("{display_width} x {display_height}"),
            );
        }
        video.add_option(
            "Aspect",
            self.get_double("video-params/aspect")?
                .filter(|aspect| aspect.is_finite() && *aspect > 0.0)
                .map(format_aspect_ratio),
        );
        video.add_option(
            "Container FPS",
            self.get_double("container-fps")?
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .map(format_fps),
        );
        video.add_option(
            "Estimated FPS",
            self.get_double("estimated-vf-fps")?
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .map(format_fps),
        );
        video.add_option("Pixel Format", self.get_string("video-params/pixelformat")?);
        video.add_option(
            "Hardware Format",
            self.get_string("video-params/hw-pixelformat")?,
        );
        video.add_option("Color Space", self.get_string("video-params/colormatrix")?);
        video.add_option("Levels", self.get_string("video-params/colorlevels")?);
        video.add_option("Transfer", self.get_string("video-params/gamma")?);
        video.add_option("Primaries", self.get_string("video-params/primaries")?);
        video.add_option(
            "Chroma Location",
            self.get_string("video-params/chroma-location")?,
        );
        video.add_option(
            "Signal Peak",
            self.get_double("video-params/sig-peak")?
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|value| format!("{value:.3}")),
        );
        video.add_option(
            "Peak Luminance",
            self.get_double("video-params/max-luma")?
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|value| format!("{value:.0} nits")),
        );
        video.add_option(
            "Rotation",
            self.get_i64("video-params/rotate")?
                .filter(|value| *value != 0)
                .map(|value| format!("{value} deg")),
        );
        push_section(&mut sections, video);

        let mut audio = InfoSection::new("Audio");
        audio.add_option(
            "Codec",
            self.get_string("audio-codec")?
                .map(|codec| friendly_codec(&codec)),
        );
        if let Some(prefix) = self.selected_track_prefix("audio")? {
            audio.add_option("Track", selected_track_title(self, &prefix)?);
            audio.add_option("Language", self.get_string(&format!("{prefix}/lang"))?);
            audio.add_option(
                "Channels",
                self.get_string(&format!("{prefix}/audio-channels"))?,
            );
            audio.add_option(
                "Sample Rate",
                self.get_i64(&format!("{prefix}/demux-samplerate"))?
                    .filter(|sample_rate| *sample_rate > 0)
                    .map(format_sample_rate),
            );
            audio.add_option(
                "Bitrate",
                self.get_i64(&format!("{prefix}/demux-bitrate"))?
                    .filter(|bitrate| *bitrate > 0)
                    .map(format_bitrate),
            );
        }
        audio.add_option("Output Format", self.get_string("audio-params/format")?);
        audio.add_option(
            "Output Channels",
            self.get_string("audio-params/hr-channels")?,
        );
        audio.add_option(
            "Output Rate",
            self.get_i64("audio-params/samplerate")?
                .filter(|sample_rate| *sample_rate > 0)
                .map(format_sample_rate),
        );
        push_section(&mut sections, audio);

        let chapters = self.chapters()?;
        let mut chapter_section = InfoSection::new("Chapters");
        if !chapters.is_empty() {
            chapter_section.add("Count", chapters.len().to_string());
            if let Some(first) = chapters.first() {
                chapter_section.add(
                    "First",
                    first
                        .title
                        .as_deref()
                        .filter(|title| !title.is_empty())
                        .map(|title| format!("{} ({})", title, format_duration(first.time)))
                        .unwrap_or_else(|| format_duration(first.time)),
                );
            }
        }
        push_section(&mut sections, chapter_section);

        let mut stats = InfoSection::new("Playback");
        stats.add_option("Hardware Decode", self.get_string("hwdec-current")?);
        stats.add_option("Video Output", self.get_string("current-vo")?);
        stats.add_option("Scaler", self.get_string("scale")?);
        stats.add_option("Tone Mapping", self.get_string("tone-mapping")?);
        stats.add_option("Sync Mode", self.get_string("video-sync")?);
        stats.add_option(
            "A/V Sync",
            self.get_double("avsync")?
                .filter(|value| value.is_finite())
                .map(|value| format!("{value:+.3} s")),
        );
        stats.add_option(
            "Dropped Frames",
            self.get_i64("frame-drop-count")?
                .filter(|value| *value >= 0)
                .map(|value| value.to_string()),
        );
        stats.add_option(
            "Cache",
            self.get_double("demuxer-cache-duration")?
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| format!("{value:.1} s")),
        );
        stats.add_option(
            "Display FPS",
            self.get_double("display-fps")?
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .map(format_fps),
        );
        push_section(&mut sections, stats);

        Ok(MediaInfo {
            title,
            path: path_text,
            sections,
            tracks: self.info_tracks()?,
        })
    }

    pub fn cycle_pause(&self) -> Result<(), MpvError> {
        self.command(&["cycle", "pause"])
    }

    pub fn stop(&self) -> Result<(), MpvError> {
        self.command(&["stop"])
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

    pub fn speed(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("speed")?.unwrap_or(1.0))
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), MpvError> {
        self.set_double("speed", speed.clamp(0.25, 4.0))
    }

    pub fn set_brightness(&self, value: f64) -> Result<(), MpvError> {
        self.set_double("brightness", video_adjustment(value))
    }

    pub fn set_contrast(&self, value: f64) -> Result<(), MpvError> {
        self.set_double("contrast", video_adjustment(value))
    }

    pub fn set_saturation(&self, value: f64) -> Result<(), MpvError> {
        self.set_double("saturation", video_adjustment(value))
    }

    pub fn set_gamma(&self, value: f64) -> Result<(), MpvError> {
        self.set_double("gamma", video_adjustment(value))
    }

    pub fn set_video_adjustments(
        &self,
        brightness: f64,
        contrast: f64,
        saturation: f64,
        gamma: f64,
    ) -> Result<(), MpvError> {
        self.set_brightness(brightness)?;
        self.set_contrast(contrast)?;
        self.set_saturation(saturation)?;
        self.set_gamma(gamma)
    }

    pub fn set_audio_normalization(&self, enabled: bool) -> Result<(), MpvError> {
        let _ = self.command(&["af", "remove", AUDIO_NORMALIZATION_FILTER_LABEL]);
        if enabled {
            self.command(&["af", "add", AUDIO_NORMALIZATION_FILTER])
        } else {
            Ok(())
        }
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

    fn selected_track_prefix(&self, kind: &str) -> Result<Option<String>, MpvError> {
        let count = self.get_i64("track-list/count")?.unwrap_or(0).max(0);
        for index in 0..count {
            let prefix = format!("track-list/{index}");
            if self.get_string(&format!("{prefix}/type"))?.as_deref() == Some(kind)
                && self
                    .get_flag(&format!("{prefix}/selected"))?
                    .unwrap_or(false)
            {
                return Ok(Some(prefix));
            }
        }

        Ok(None)
    }

    fn info_tracks(&self) -> Result<Vec<InfoTrack>, MpvError> {
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

            let id = self.get_i64(&format!("{prefix}/id"))?.unwrap_or(0);
            let title = self
                .get_string(&format!("{prefix}/title"))?
                .or(self.get_string(&format!("{prefix}/lang"))?)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| format!("Track {id}"));
            let codec = self.get_string(&format!("{prefix}/codec"))?;
            let language = self.get_string(&format!("{prefix}/lang"))?;
            let external = self
                .get_flag(&format!("{prefix}/external"))?
                .unwrap_or(false);
            let default = self
                .get_flag(&format!("{prefix}/default"))?
                .unwrap_or(false);
            let selected = self
                .get_flag(&format!("{prefix}/selected"))?
                .unwrap_or(false);

            let mut details = Vec::new();
            if selected {
                details.push("Selected".to_owned());
            }
            if let Some(language) = language {
                details.push(language);
            }
            if let Some(codec) = codec {
                details.push(friendly_codec(&codec));
            }
            if kind == TrackKind::Audio {
                if let Some(channels) = self.get_string(&format!("{prefix}/audio-channels"))? {
                    details.push(channels);
                }
                if let Some(sample_rate) = self
                    .get_i64(&format!("{prefix}/demux-samplerate"))?
                    .filter(|sample_rate| *sample_rate > 0)
                {
                    details.push(format_sample_rate(sample_rate));
                }
                if let Some(bitrate) = self
                    .get_i64(&format!("{prefix}/demux-bitrate"))?
                    .filter(|bitrate| *bitrate > 0)
                {
                    details.push(format_bitrate(bitrate));
                }
            }
            if external {
                details.push("External".to_owned());
            }
            if default {
                details.push("Default".to_owned());
            }

            tracks.push(InfoTrack {
                id,
                kind,
                selected,
                external,
                default,
                title,
                detail: details.join(" · "),
            });
        }

        Ok(tracks)
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

fn push_section(sections: &mut Vec<InfoSection>, section: InfoSection) {
    if !section.rows.is_empty() {
        sections.push(section);
    }
}

fn display_path_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .filter(|name| !name.is_empty())
        .map(|name| name.into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn selected_track_title(mpv: &Mpv, prefix: &str) -> Result<Option<String>, MpvError> {
    let id = mpv.get_i64(&format!("{prefix}/id"))?.unwrap_or(0);
    Ok(mpv
        .get_string(&format!("{prefix}/title"))?
        .or(mpv.get_string(&format!("{prefix}/lang"))?)
        .filter(|title| !title.is_empty())
        .or_else(|| (id > 0).then(|| format!("Track {id}"))))
}

fn friendly_container(container: &str) -> String {
    match container.to_ascii_lowercase().as_str() {
        "matroska,webm" | "matroska" => "Matroska / WebM".to_owned(),
        "mov,mp4,m4a,3gp,3g2,mj2" => "MP4 / QuickTime".to_owned(),
        "avi" => "AVI".to_owned(),
        "mpegts" => "MPEG-TS".to_owned(),
        value => value.to_owned(),
    }
}

fn friendly_codec(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "h264" | "avc1" => "H.264 / AVC".to_owned(),
        "hevc" | "h265" => "H.265 / HEVC".to_owned(),
        "av1" => "AV1".to_owned(),
        "vp8" => "VP8".to_owned(),
        "vp9" => "VP9".to_owned(),
        "aac" => "AAC".to_owned(),
        "ac3" => "AC-3".to_owned(),
        "eac3" => "E-AC-3".to_owned(),
        "truehd" => "Dolby TrueHD".to_owned(),
        "dts" => "DTS".to_owned(),
        "flac" => "FLAC".to_owned(),
        "mp3" => "MP3".to_owned(),
        "opus" => "Opus".to_owned(),
        "vorbis" => "Vorbis".to_owned(),
        "ass" => "ASS".to_owned(),
        "subrip" | "srt" => "SRT".to_owned(),
        "webvtt" => "WebVTT".to_owned(),
        value => value.to_ascii_uppercase(),
    }
}

fn format_bytes(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", value as i64, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_bitrate(bits_per_second: i64) -> String {
    if bits_per_second >= 1_000_000 {
        format!("{:.1} Mbps", bits_per_second as f64 / 1_000_000.0)
    } else {
        format!("{:.0} kbps", bits_per_second as f64 / 1_000.0)
    }
}

fn format_sample_rate(hertz: i64) -> String {
    if hertz >= 1000 {
        format!("{:.1} kHz", hertz as f64 / 1000.0)
    } else {
        format!("{hertz} Hz")
    }
}

fn format_fps(fps: f64) -> String {
    format!("{fps:.3} fps")
}

fn video_adjustment(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-100.0, 100.0)
    } else {
        0.0
    }
}

fn format_aspect_ratio(aspect: f64) -> String {
    const COMMON: [(u32, u32); 5] = [(4, 3), (16, 9), (16, 10), (21, 9), (64, 27)];
    for (width, height) in COMMON {
        let common = f64::from(width) / f64::from(height);
        if (aspect - common).abs() < 0.01 {
            return format!("{width}:{height}");
        }
    }

    format!("{aspect:.3}:1")
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_owned();
    }

    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
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

pub fn resolve_render_target_size(
    viewport: Option<RenderTargetSize>,
    resize: Option<RenderTargetSize>,
    widget_width: i32,
    widget_height: i32,
    scale_factor: i32,
) -> RenderTargetSize {
    let widget_size = RenderTargetSize {
        width: widget_width.max(1),
        height: widget_height.max(1),
    };
    let scale_factor = scale_factor.max(1);
    let scaled_widget_size = RenderTargetSize {
        width: widget_size.width.saturating_mul(scale_factor),
        height: widget_size.height.saturating_mul(scale_factor),
    };

    let mut target = resize
        .filter(|size| size.is_valid())
        .unwrap_or(widget_size)
        .max_components(scaled_widget_size);

    if let Some(viewport) = viewport.filter(|size| size.is_valid())
        && viewport.width >= target.width
        && viewport.height >= target.height
        && viewport.area() >= target.area()
    {
        target = viewport;
    }

    target
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_media_sizes() {
        assert_eq!(format_bytes(42), "42 B");
        assert_eq!(format_bytes(1_572_864), "1.5 MB");
        assert_eq!(format_bytes(3_221_225_472), "3.0 GB");
    }

    #[test]
    fn formats_media_duration() {
        assert_eq!(format_duration(0.0), "00:00");
        assert_eq!(format_duration(125.2), "02:05");
        assert_eq!(format_duration(6906.0), "01:55:06");
    }

    #[test]
    fn formats_common_aspect_ratios() {
        assert_eq!(format_aspect_ratio(16.0 / 9.0), "16:9");
        assert_eq!(format_aspect_ratio(4.0 / 3.0), "4:3");
        assert_eq!(format_aspect_ratio(2.0), "2.000:1");
    }

    #[test]
    fn clamps_video_adjustments() {
        assert_eq!(video_adjustment(125.0), 100.0);
        assert_eq!(video_adjustment(-125.0), -100.0);
        assert_eq!(video_adjustment(f64::NAN), 0.0);
    }

    #[test]
    fn audio_normalization_filter_is_labelled() {
        assert_eq!(AUDIO_NORMALIZATION_FILTER_LABEL, "@okpnorm");
        assert_eq!(AUDIO_NORMALIZATION_FILTER, "@okpnorm:dynaudnorm");
    }

    #[test]
    fn resolves_hidpi_logical_resize_to_scaled_widget_size() {
        let target = resolve_render_target_size(
            None,
            Some(RenderTargetSize {
                width: 1024,
                height: 576,
            }),
            1024,
            576,
            2,
        );

        assert_eq!(
            target,
            RenderTargetSize {
                width: 2048,
                height: 1152,
            }
        );
    }

    #[test]
    fn keeps_physical_resize_size_when_it_matches_scaled_widget() {
        let target = resolve_render_target_size(
            None,
            Some(RenderTargetSize {
                width: 2048,
                height: 1152,
            }),
            1024,
            576,
            2,
        );

        assert_eq!(
            target,
            RenderTargetSize {
                width: 2048,
                height: 1152,
            }
        );
    }

    #[test]
    fn prefers_larger_gl_viewport_for_fractional_or_backend_scaling() {
        let target = resolve_render_target_size(
            Some(RenderTargetSize {
                width: 1536,
                height: 864,
            }),
            Some(RenderTargetSize {
                width: 1024,
                height: 576,
            }),
            1024,
            576,
            1,
        );

        assert_eq!(
            target,
            RenderTargetSize {
                width: 1536,
                height: 864,
            }
        );
    }

    #[test]
    fn ignores_too_small_gl_viewport_snapshot() {
        let target = resolve_render_target_size(
            Some(RenderTargetSize {
                width: 640,
                height: 360,
            }),
            Some(RenderTargetSize {
                width: 1024,
                height: 576,
            }),
            1024,
            576,
            2,
        );

        assert_eq!(
            target,
            RenderTargetSize {
                width: 2048,
                height: 1152,
            }
        );
    }

    #[test]
    fn expands_common_codec_names() {
        assert_eq!(friendly_codec("h264"), "H.264 / AVC");
        assert_eq!(friendly_codec("eac3"), "E-AC-3");
        assert_eq!(friendly_codec("subrip"), "SRT");
    }
}
