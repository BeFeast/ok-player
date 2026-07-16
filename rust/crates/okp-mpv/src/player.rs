use std::any::Any;
use std::ffi::{CStr, CString, NulError};
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use libc::{c_char, c_int, c_void};
use thiserror::Error;

use crate::ffi;
use crate::pump::EventPump;

const AUDIO_NORMALIZATION_FILTER_LABEL: &str = "@okpnorm";
const AUDIO_NORMALIZATION_FILTER: &str = "@okpnorm:dynaudnorm";
const AUDIO_DEVICE_AUTO: &str = "auto";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderUpdateHandle {
    context: NonNull<ffi::mpv_render_context>,
}

// libmpv allows the embedding application to call render-context update from
// its selected render thread. GTK schedules this handle back onto the main
// context and drops pending callbacks before freeing the render context.
unsafe impl Send for RenderUpdateHandle {}
unsafe impl Sync for RenderUpdateHandle {}

/// An opaque native Wayland display kept alive for an mpv render context.
///
/// The owner is type-erased so shells can retain their toolkit display object
/// without making `okp-mpv` depend on toolkit-specific types.
pub struct NativeWaylandDisplay {
    pointer: NonNull<c_void>,
    _owner: Box<dyn Any>,
}

impl NativeWaylandDisplay {
    /// Wrap a native `wl_display*` together with the resource that owns it.
    ///
    /// # Safety
    ///
    /// `pointer` must identify the `wl_display` owned by `owner`, and it must
    /// remain valid for as long as retaining `owner` keeps that display alive.
    pub unsafe fn new<T: 'static>(pointer: NonNull<c_void>, owner: T) -> Self {
        Self {
            pointer,
            _owner: Box::new(owner),
        }
    }

    fn pointer(&self) -> NonNull<c_void> {
        self.pointer
    }
}

impl fmt::Debug for NativeWaylandDisplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeWaylandDisplay")
            .field("pointer", &self.pointer)
            .finish_non_exhaustive()
    }
}

impl RenderUpdateHandle {
    pub fn update_has_frame(self) -> bool {
        let flags = unsafe { ffi::mpv_render_context_update(self.context.as_ptr()) };
        flags & ffi::MPV_RENDER_UPDATE_FRAME != 0
    }
}

/// Display dimensions carried by lifecycle events after mpv has applied pixel
/// aspect and rotation. Shells consume this payload instead of issuing a
/// blocking property read from their UI thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoDimensions {
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
    /// Seconds currently cached ahead of the playhead. Observed by the event
    /// pump so shells can render a buffered timeline without a UI-thread read.
    pub cache_duration: Option<f64>,
    /// Container frame rate, present only for video with a declared FPS. Feeds
    /// the transient seek/frame-step readout (PRD P4-N4); `None` for audio-only
    /// or frame-rate-less sources, which then show a timecode without a frame.
    pub container_fps: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct AbLoopState {
    pub a: Option<f64>,
    pub b: Option<f64>,
}

impl AbLoopState {
    pub fn is_active(self) -> bool {
        self.a.is_some() || self.b.is_some()
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub label: String,
    pub selected: bool,
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

/// Lifecycle events the engine fires, drained oldest-first via
/// [`Mpv::take_lifecycle_events`](crate::Mpv::take_lifecycle_events). Load and
/// reconfiguration events carry display dimensions read by the background pump,
/// while `EndFile` carries the path mpv reported for the entry that ended. The
/// shell can therefore react without a blocking UI-thread property read and can
/// drop a stale error whose source has already been superseded. Not `Copy`
/// because `path` is a `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpvEvent {
    EndFile {
        reason: EndFileReason,
        path: Option<String>,
    },
    CommandReply {
        request_id: u64,
        error: c_int,
    },
    FileLoaded {
        video_dimensions: Option<VideoDimensions>,
    },
    VideoReconfig {
        video_dimensions: Option<VideoDimensions>,
    },
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

/// A bare, non-owning handle over which every mpv property read is issued.
///
/// It carries no debug guard, so it is safe to use from the background event
/// pump: the guard exists to catch reads on the *UI* thread, and the pump reads
/// off it deliberately. `Mpv` owns the handle and its destruction; `RawReader`
/// only borrows the pointer, which the libmpv client API allows to be used from
/// any thread concurrently with commands and rendering.
#[derive(Clone, Copy)]
pub(crate) struct RawReader {
    handle: NonNull<ffi::mpv_handle>,
}

// SAFETY: the libmpv client API (mpv_get_property/mpv_command/mpv_wait_event/…)
// is documented as thread-safe; the render API used on the UI thread is a
// separate, independently thread-safe surface. `RawReader` never touches the
// render context.
unsafe impl Send for RawReader {}
unsafe impl Sync for RawReader {}

impl RawReader {
    pub(crate) fn new(handle: NonNull<ffi::mpv_handle>) -> Self {
        Self { handle }
    }

    pub(crate) fn handle(&self) -> NonNull<ffi::mpv_handle> {
        self.handle
    }

    /// The path/URL mpv reports for the current entry — the file path for local media
    /// or the URL string for a stream. Read by the pump at `EndFile` time so the
    /// shell can match the ended source against the current one and drop a stale
    /// `EndFile::Error` whose source was superseded. `None` when mpv has no current
    /// entry (e.g. it cleared `path` before the pump read it).
    pub(crate) fn path(&self) -> Option<String> {
        self.get_string("path").ok().flatten()
    }

    pub(crate) fn playback_state(&self) -> Result<PlaybackState, MpvError> {
        Ok(PlaybackState {
            time_pos: self.get_double("time-pos")?,
            duration: self.get_double("duration")?,
            paused: self.get_flag("pause")?.unwrap_or(false),
            volume: self.get_double("volume")?,
            speed: self.get_double("speed")?,
            cache_duration: self
                .get_double("demuxer-cache-duration")?
                .filter(|value| value.is_finite() && *value >= 0.0),
            container_fps: self
                .get_double("container-fps")?
                .filter(|fps| fps.is_finite() && *fps > 0.0),
        })
    }

    pub(crate) fn video_dimensions(&self) -> Result<Option<VideoDimensions>, MpvError> {
        let width =
            self.first_positive_i64(&["video-params/dw", "dwidth", "video-params/w", "width"])?;
        let height =
            self.first_positive_i64(&["video-params/dh", "dheight", "video-params/h", "height"])?;

        Ok(match (width, height) {
            (Some(width), Some(height)) => match (i32::try_from(width), i32::try_from(height)) {
                (Ok(width), Ok(height)) => Some(VideoDimensions { width, height }),
                _ => None,
            },
            _ => None,
        })
    }

    fn first_positive_i64(&self, names: &[&str]) -> Result<Option<i64>, MpvError> {
        for name in names {
            if let Some(value) = self.get_i64(name)?
                && value > 0
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub(crate) fn ab_loop_state(&self) -> Result<AbLoopState, MpvError> {
        Ok(AbLoopState {
            a: self
                .get_string("ab-loop-a")?
                .as_deref()
                .and_then(parse_ab_loop_point),
            b: self
                .get_string("ab-loop-b")?
                .as_deref()
                .and_then(parse_ab_loop_point),
        })
    }

    pub(crate) fn secondary_subtitle_id(&self) -> Result<Option<i64>, MpvError> {
        Ok(self.get_i64("secondary-sid")?.filter(|id| *id > 0))
    }

    pub(crate) fn subtitle_delay(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("sub-delay")?.unwrap_or(0.0))
    }

    pub(crate) fn audio_delay(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("audio-delay")?.unwrap_or(0.0))
    }

    pub(crate) fn subtitle_scale(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("sub-scale")?.unwrap_or(1.0))
    }

    pub(crate) fn speed(&self) -> Result<f64, MpvError> {
        Ok(self.get_double("speed")?.unwrap_or(1.0))
    }

    pub(crate) fn tracks(&self) -> Result<Vec<Track>, MpvError> {
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

    pub(crate) fn audio_devices(&self) -> Result<Vec<AudioDevice>, MpvError> {
        let count = self.get_i64("audio-device-list/count")?.unwrap_or(0).max(0);
        let current = self
            .get_string("audio-device")?
            .unwrap_or_else(|| AUDIO_DEVICE_AUTO.to_owned());
        let mut devices = Vec::new();
        let mut saw_auto = false;

        for index in 0..count {
            let prefix = format!("audio-device-list/{index}");
            let Some(name) = self.get_string(&format!("{prefix}/name"))? else {
                continue;
            };
            if name == AUDIO_DEVICE_AUTO {
                saw_auto = true;
            }
            devices.push(AudioDevice {
                selected: audio_device_selected(&name, &current),
                label: audio_device_label(
                    &name,
                    self.get_string(&format!("{prefix}/description"))?,
                ),
                name,
            });
        }

        if !saw_auto {
            devices.insert(
                0,
                AudioDevice {
                    name: AUDIO_DEVICE_AUTO.to_owned(),
                    label: "Automatic".to_owned(),
                    selected: audio_device_selected(AUDIO_DEVICE_AUTO, &current),
                },
            );
        }

        Ok(devices)
    }

    pub(crate) fn chapters(&self) -> Result<Vec<Chapter>, MpvError> {
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

    pub(crate) fn media_info(&self, path: Option<&Path>) -> Result<MediaInfo, MpvError> {
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
        let pixel_format = self.get_string("video-params/pixelformat")?;
        video.add_option("Pixel Format", pixel_format.clone());
        if let Some(bit_depth) = pixel_format
            .as_deref()
            .and_then(bit_depth_from_pixel_format)
        {
            video.add("Bit Depth", format!("{bit_depth}-bit"));
        }
        video.add_option(
            "Hardware Format",
            self.get_string("video-params/hw-pixelformat")?,
        );
        video.add_option(
            "Color Space",
            self.get_string("video-params/colormatrix")?
                .map(|value| friendly_color_matrix(&value)),
        );
        video.add_option(
            "Levels",
            self.get_string("video-params/colorlevels")?
                .map(|value| friendly_color_levels(&value)),
        );
        let transfer = self.get_string("video-params/gamma")?;
        let primaries = self.get_string("video-params/primaries")?;
        let signal_peak = self
            .get_double("video-params/sig-peak")?
            .filter(|value| value.is_finite() && *value > 0.0);
        let peak_luminance = self
            .get_double("video-params/max-luma")?
            .filter(|value| value.is_finite() && *value > 0.0);
        video.add_option(
            "Dynamic Range",
            dynamic_range_summary(
                transfer.as_deref(),
                primaries.as_deref(),
                signal_peak,
                peak_luminance,
            ),
        );
        video.add_option("Transfer", transfer.map(|value| friendly_transfer(&value)));
        video.add_option(
            "Primaries",
            primaries.map(|value| friendly_primaries(&value)),
        );
        video.add_option(
            "Chroma Location",
            self.get_string("video-params/chroma-location")?,
        );
        video.add_option(
            "Signal Peak",
            signal_peak.map(|value| format!("{value:.3}")),
        );
        video.add_option(
            "Peak Luminance",
            peak_luminance.map(|value| format!("{value:.0} nits")),
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
        // mpv flags both the primary and the secondary caption as `selected`, so
        // read `secondary-sid` to name each subtitle slot explicitly instead of
        // a bare "Selected" that would read the same on both.
        let secondary_sid = self.secondary_subtitle_id()?;
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
            if kind == TrackKind::Subtitle {
                // Distinguish the two caption slots on the media surface; the
                // secondary is matched by id since its `selected` flag is set too.
                if secondary_sid == Some(id) {
                    details.push("Secondary".to_owned());
                } else if selected {
                    details.push("Primary".to_owned());
                }
            } else if selected {
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
}

fn render_context_parameters(
    api: &CStr,
    init_params: &mut ffi::mpv_opengl_init_params,
    native_wayland_display: Option<NonNull<c_void>>,
) -> Vec<ffi::mpv_render_param> {
    let mut params = vec![
        ffi::mpv_render_param {
            param_type: ffi::MPV_RENDER_PARAM_API_TYPE,
            data: api.as_ptr().cast_mut().cast(),
        },
        ffi::mpv_render_param {
            param_type: ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            data: ptr::from_mut(init_params).cast(),
        },
    ];
    if let Some(display) = native_wayland_display {
        params.push(ffi::mpv_render_param {
            param_type: ffi::MPV_RENDER_PARAM_WL_DISPLAY,
            data: display.as_ptr(),
        });
    }
    params.push(ffi::mpv_render_param {
        param_type: ffi::MPV_RENDER_PARAM_INVALID,
        data: ptr::null_mut(),
    });
    params
}

pub struct Mpv {
    handle: NonNull<ffi::mpv_handle>,
    render_context: Option<NonNull<ffi::mpv_render_context>>,
    // Must be released after `mpv_render_context_free`: libmpv may use the
    // native display until that call returns.
    render_context_native_wayland_display: Option<NativeWaylandDisplay>,
    pump: Option<EventPump>,
    next_request_id: AtomicU64,
    #[cfg(debug_assertions)]
    blocking_read_guard: crate::guard::BlockingReadGuard,
}

impl Mpv {
    pub fn new() -> Result<Self, MpvError> {
        Self::new_with_options("no", &[])
    }

    pub fn new_with_hwdec(hwdec: &str) -> Result<Self, MpvError> {
        Self::new_with_options(hwdec, &[])
    }

    pub fn new_with_options(hwdec: &str, options: &[(String, String)]) -> Result<Self, MpvError> {
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
        }

        let handle = NonNull::new(unsafe { ffi::mpv_create() }).ok_or(MpvError::NullHandle)?;
        let this = Self {
            handle,
            render_context: None,
            render_context_native_wayland_display: None,
            pump: None,
            next_request_id: AtomicU64::new(1),
            #[cfg(debug_assertions)]
            blocking_read_guard: Default::default(),
        };

        this.set_option("terminal", "no")?;
        this.set_option("config", "no")?;
        this.set_option("idle", "yes")?;
        this.set_option("force-window", "no")?;
        this.set_option("vo", "libmpv")?;
        this.set_option("hwdec", hwdec)?;
        // Exact same-stem subtitle discovery is an mpv passthrough boundary:
        // libmpv parses and renders SRT/WebVTT cue payloads, while OK Player
        // only surfaces the resulting track metadata. Keep this explicit so
        // config=no cannot make sidecar support depend on mpv's default value.
        this.set_option("sub-auto", "exact")?;
        this.apply_options(options)?;
        check(unsafe { ffi::mpv_initialize(this.handle.as_ptr()) })?;

        Ok(this)
    }

    /// Mark the calling thread as the UI (GLib main-context) thread. In debug
    /// builds, every later blocking property read issued from this thread is
    /// counted and hard-logged with a backtrace — the Rust twin of the Windows
    /// DEBUG render-thread guard (see `guard` for why it logs instead of
    /// aborting). No-op in release builds.
    pub fn mark_ui_thread(&self) {
        #[cfg(debug_assertions)]
        self.blocking_read_guard.mark_ui_thread();
    }

    /// Number of blocking property reads issued from the marked UI thread.
    /// Debug builds only; exists so tests can assert the tripwire fires.
    #[cfg(debug_assertions)]
    pub fn blocking_read_violations(&self) -> usize {
        self.blocking_read_guard.violations()
    }

    /// Start the background event pump: observe the properties the shell cares
    /// about, register the wakeup callback, and spawn the thread that reads
    /// state off the UI thread. Idempotent; call once after the handle is
    /// created (and, in the shell, after `mark_ui_thread`).
    pub fn start_event_pump(&mut self) {
        if self.pump.is_none() {
            self.pump = Some(EventPump::start(self.handle));
        }
    }

    fn reader(&self) -> RawReader {
        RawReader::new(self.handle)
    }

    /// Read the live playback scalars synchronously. This is a blocking mpv
    /// call and trips the debug guard on the marked UI thread — the shell reads
    /// from [`Mpv::observed_playback_state`] instead. Kept as the guarded read
    /// the tripwire test exercises and the regression backstop for new callers.
    pub fn playback_state(&self) -> Result<PlaybackState, MpvError> {
        #[cfg(debug_assertions)]
        self.blocking_read_guard.check_blocking_read("time-pos");
        self.reader().playback_state()
    }

    /// Latest playback scalars observed by the pump. A plain in-memory read; no
    /// mpv call, safe from the UI thread.
    pub fn observed_playback_state(&self) -> PlaybackState {
        self.pump
            .as_ref()
            .map(EventPump::playback_state)
            .unwrap_or_default()
    }

    pub fn observed_ab_loop_state(&self) -> AbLoopState {
        self.pump
            .as_ref()
            .map(EventPump::ab_loop_state)
            .unwrap_or_default()
    }

    pub fn observed_subtitle_delay(&self) -> f64 {
        self.pump
            .as_ref()
            .map(EventPump::subtitle_delay)
            .unwrap_or(0.0)
    }

    pub fn observed_audio_delay(&self) -> f64 {
        self.pump
            .as_ref()
            .map(EventPump::audio_delay)
            .unwrap_or(0.0)
    }

    pub fn observed_subtitle_scale(&self) -> f64 {
        self.pump
            .as_ref()
            .map(EventPump::subtitle_scale)
            .unwrap_or(1.0)
    }

    pub fn observed_speed(&self) -> f64 {
        self.pump.as_ref().map(EventPump::speed).unwrap_or(1.0)
    }

    pub fn observed_secondary_subtitle_id(&self) -> Option<i64> {
        self.pump
            .as_ref()
            .and_then(EventPump::secondary_subtitle_id)
    }

    pub fn observed_chapters(&self) -> Vec<Chapter> {
        self.pump
            .as_ref()
            .map(EventPump::chapters)
            .unwrap_or_default()
    }

    pub fn observed_tracks(&self) -> Vec<Track> {
        self.pump
            .as_ref()
            .map(EventPump::tracks)
            .unwrap_or_default()
    }

    pub fn observed_audio_devices(&self) -> Vec<AudioDevice> {
        self.pump
            .as_ref()
            .map(EventPump::audio_devices)
            .unwrap_or_default()
    }

    pub fn observed_media_info(&self) -> Option<MediaInfo> {
        self.pump.as_ref().and_then(EventPump::media_info)
    }

    /// Drain the lifecycle events (`FileLoaded`/`EndFile`/`Shutdown`) the pump
    /// has queued since the last call, oldest first.
    pub fn take_lifecycle_events(&self) -> Vec<MpvEvent> {
        self.pump
            .as_ref()
            .map(EventPump::take_lifecycle_events)
            .unwrap_or_default()
    }

    /// Tell the pump which local path backs the current media so `media-info`
    /// reports the same title/path the shell used to pass synchronously.
    pub fn set_media_source(&self, source: Option<PathBuf>) {
        if let Some(pump) = self.pump.as_ref() {
            pump.set_media_source(source);
        }
    }

    /// Create the OpenGL render context, optionally enabling Wayland native
    /// display interop for direct hardware decoding.
    ///
    /// When supplied, the display resource is retained until
    /// [`Self::destroy_render_context`] frees the libmpv render context.
    pub fn create_render_context(
        &mut self,
        native_wayland_display: Option<NativeWaylandDisplay>,
    ) -> Result<(), MpvError> {
        if self.render_context.is_some() {
            return Ok(());
        }

        let api = CString::new("opengl")?;
        let mut init_params = ffi::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = render_context_parameters(
            &api,
            &mut init_params,
            native_wayland_display
                .as_ref()
                .map(NativeWaylandDisplay::pointer),
        );

        let mut context = ptr::null_mut();
        check(unsafe {
            ffi::mpv_render_context_create(&mut context, self.handle.as_ptr(), params.as_mut_ptr())
        })?;
        self.render_context = NonNull::new(context);
        if self.render_context.is_some() {
            self.render_context_native_wayland_display = native_wayland_display;
        }

        Ok(())
    }

    pub fn render_update_handle(&self) -> Result<RenderUpdateHandle, MpvError> {
        Ok(RenderUpdateHandle {
            context: self.render_context.ok_or(MpvError::MissingRenderContext)?,
        })
    }

    /// Install libmpv's render update callback.
    ///
    /// # Safety
    ///
    /// `callback_ctx` must remain valid until this method is called again with
    /// `None` or the render context is destroyed. The callback must not call
    /// back into mpv; it may only wake the render/UI thread.
    pub unsafe fn set_render_update_callback(
        &mut self,
        callback: Option<unsafe extern "C" fn(*mut c_void)>,
        callback_ctx: *mut c_void,
    ) -> Result<(), MpvError> {
        let context = self
            .render_context
            .ok_or(MpvError::MissingRenderContext)?
            .as_ptr();
        unsafe {
            ffi::mpv_render_context_set_update_callback(context, callback, callback_ctx);
        }
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

    pub fn apply_options(&self, options: &[(String, String)]) -> Result<(), MpvError> {
        for (name, value) in options {
            self.set_option(name, value)?;
        }

        Ok(())
    }

    pub fn cycle_pause(&self) -> Result<(), MpvError> {
        self.command_async(&["cycle", "pause"])
    }

    pub fn stop(&self) -> Result<(), MpvError> {
        self.command_async(&["stop"])
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), MpvError> {
        let seconds = seconds.max(0.0).to_string();
        self.command_async(&["seek", &seconds, "absolute+exact"])
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), MpvError> {
        self.command_async(&["seek", &seconds.to_string(), "relative+exact"])
    }

    pub fn frame_step(&self) -> Result<(), MpvError> {
        self.command_async(&["frame-step"])
    }

    pub fn frame_back_step(&self) -> Result<(), MpvError> {
        self.command_async(&["frame-back-step"])
    }

    pub fn toggle_ab_loop(&self) -> Result<(), MpvError> {
        self.command(&["ab-loop"])
    }

    pub fn screenshot_to_file_async(
        &self,
        path: &Path,
        include_subtitles: bool,
    ) -> Result<u64, MpvError> {
        let path = path.to_string_lossy();
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        self.command_async_with_userdata(
            &[
                "screenshot-to-file",
                &path,
                screenshot_mode(include_subtitles),
            ],
            request_id,
        )?;
        Ok(request_id)
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), MpvError> {
        self.set_double("volume", volume.clamp(0.0, 130.0))
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

    pub fn set_video_aspect_override(&self, value: &str) -> Result<(), MpvError> {
        self.command(&["set", "video-aspect-override", video_aspect_override(value)])
    }

    pub fn set_video_rotation(&self, degrees: i64) -> Result<(), MpvError> {
        let degrees = normalized_video_rotation(degrees).to_string();
        self.command(&["set", "video-rotate", &degrees])
    }

    pub fn set_video_fill_screen(&self, enabled: bool) -> Result<(), MpvError> {
        self.set_double("panscan", if enabled { 1.0 } else { 0.0 })
    }

    pub fn reset_video_transform(&self) -> Result<(), MpvError> {
        self.set_video_rotation(0)?;
        self.set_video_fill_screen(false)?;
        self.set_video_aspect_override("no")
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

    pub fn select_audio(&self, id: Option<i64>) -> Result<(), MpvError> {
        let value = track_id_or_off(id);
        self.command(&["set", "aid", &value])
    }

    pub fn set_audio_device(&self, name: &str) -> Result<(), MpvError> {
        self.command(&["set", "audio-device", normalized_audio_device_name(name)])
    }

    /// Restore a saved audio output if it is present in the observed device
    /// list. Reads the pump snapshot (no blocking mpv call) and only issues the
    /// `set` command when the device is available.
    pub fn restore_audio_device(&self, name: &str) -> Result<bool, MpvError> {
        let name = normalized_audio_device_name(name);
        if name == AUDIO_DEVICE_AUTO {
            return Ok(false);
        }

        if self
            .observed_audio_devices()
            .iter()
            .any(|device| device.name == name)
        {
            self.set_audio_device(name)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn set_subtitle_delay(&self, seconds: f64) -> Result<(), MpvError> {
        self.set_double("sub-delay", seconds.clamp(-600.0, 600.0))
    }

    pub fn adjust_subtitle_delay(&self, delta_seconds: f64) -> Result<(), MpvError> {
        self.set_subtitle_delay(self.observed_subtitle_delay() + delta_seconds)
    }

    pub fn set_audio_delay(&self, seconds: f64) -> Result<(), MpvError> {
        self.set_double("audio-delay", seconds.clamp(-600.0, 600.0))
    }

    pub fn set_subtitle_scale(&self, scale: f64) -> Result<(), MpvError> {
        self.set_double("sub-scale", scale.clamp(0.25, 4.0))
    }

    pub fn set_subtitle_position(&self, position: f64) -> Result<(), MpvError> {
        self.set_double("sub-pos", position.clamp(0.0, 100.0))
    }

    pub fn adjust_subtitle_scale(&self, delta: f64) -> Result<(), MpvError> {
        self.set_subtitle_scale(self.observed_subtitle_scale() + delta)
    }

    pub fn render(&mut self, width: i32, height: i32) -> Result<(), MpvError> {
        if width <= 0 || height <= 0 {
            return Ok(());
        }

        let context = self
            .render_context
            .ok_or(MpvError::MissingRenderContext)?
            .as_ptr();

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
                ffi::mpv_render_context_set_update_callback(
                    context.as_ptr(),
                    None,
                    ptr::null_mut(),
                );
                ffi::mpv_render_context_free(context.as_ptr());
            }
        }
        self.render_context_native_wayland_display.take();
    }

    fn set_option(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;

        check(unsafe {
            ffi::mpv_set_option_string(self.handle.as_ptr(), name.as_ptr(), value.as_ptr())
        })
    }

    fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        // `_c_args` owns the CString buffers `ptrs` points into; it must outlive
        // the mpv call, so it is bound (not dropped) for the whole scope.
        let (_c_args, ptrs) = command_args(args)?;
        check(unsafe { ffi::mpv_command(self.handle.as_ptr(), ptrs.as_ptr()) })
    }

    /// Fire-and-forget command dispatch for latency-sensitive transport
    /// controls (pause/seek/frame-step). It never blocks the caller on a busy
    /// core; the reply arrives as an event the pump drains and logs on failure.
    fn command_async(&self, args: &[&str]) -> Result<(), MpvError> {
        self.command_async_with_userdata(args, 0)
    }

    fn command_async_with_userdata(&self, args: &[&str], request_id: u64) -> Result<(), MpvError> {
        let (_c_args, ptrs) = command_args(args)?;
        check(unsafe { ffi::mpv_command_async(self.handle.as_ptr(), request_id, ptrs.as_ptr()) })
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

/// Build the NUL-terminated argv libmpv expects. The `CString` vector must be
/// kept alive by the caller for as long as the pointer vector is used.
fn command_args(args: &[&str]) -> Result<(Vec<CString>, Vec<*const c_char>), MpvError> {
    let c_args = args
        .iter()
        .map(|arg| CString::new(*arg))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ptrs = c_args.iter().map(|arg| arg.as_ptr()).collect::<Vec<_>>();
    ptrs.push(ptr::null());
    Ok((c_args, ptrs))
}

fn screenshot_mode(include_subtitles: bool) -> &'static str {
    if include_subtitles {
        "subtitles"
    } else {
        "video"
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

fn selected_track_title(reader: &RawReader, prefix: &str) -> Result<Option<String>, MpvError> {
    let id = reader.get_i64(&format!("{prefix}/id"))?.unwrap_or(0);
    Ok(reader
        .get_string(&format!("{prefix}/title"))?
        .or(reader.get_string(&format!("{prefix}/lang"))?)
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

fn bit_depth_from_pixel_format(pixel_format: &str) -> Option<u8> {
    let value = pixel_format.to_ascii_lowercase();
    if value.contains("nv12")
        || value.contains("rgb24")
        || value.contains("rgba")
        || value.contains("bgra")
    {
        return Some(8);
    }
    if value.contains("p016") {
        return Some(16);
    }
    if value.contains("p012") {
        return Some(12);
    }
    if value.contains("p010") {
        return Some(10);
    }
    for depth in [16, 14, 12, 10, 9] {
        let depth = depth.to_string();
        if value.contains(&format!("p{depth}"))
            || value.contains(&format!("{depth}le"))
            || value.contains(&format!("{depth}be"))
        {
            return depth.parse().ok();
        }
    }
    if value.contains("yuv420p") || value.contains("yuv422p") || value.contains("yuv444p") {
        return Some(8);
    }
    None
}

fn dynamic_range_summary(
    transfer: Option<&str>,
    primaries: Option<&str>,
    signal_peak: Option<f64>,
    peak_luminance: Option<f64>,
) -> Option<String> {
    let transfer_label = transfer.map(friendly_transfer);
    let primaries_label = primaries.map(friendly_primaries);
    let hdr = transfer.is_some_and(is_hdr_transfer)
        || primaries.is_some_and(is_hdr_primaries)
        || signal_peak.is_some_and(|value| value > 1.1)
        || peak_luminance.is_some_and(|value| value >= 400.0);

    if hdr {
        let mut evidence = Vec::new();
        if let Some(transfer) = transfer_label.as_deref()
            && transfer != "Unknown"
        {
            evidence.push(transfer);
        }
        if let Some(primaries) = primaries_label.as_deref()
            && primaries != "Unknown"
        {
            evidence.push(primaries);
        }
        if evidence.is_empty() {
            Some("HDR".to_owned())
        } else {
            Some(format!("HDR ({})", evidence.join(", ")))
        }
    } else if transfer.is_some() || primaries.is_some() {
        Some("SDR".to_owned())
    } else {
        None
    }
}

fn is_hdr_transfer(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "pq" | "smpte2084" | "st2084" | "hlg" | "arib-std-b67"
    )
}

fn is_hdr_primaries(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "bt.2020" | "bt2020" | "bt.2100" | "bt2100"
    )
}

fn friendly_transfer(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "pq" | "smpte2084" | "st2084" => "PQ / ST 2084".to_owned(),
        "hlg" | "arib-std-b67" => "HLG".to_owned(),
        "bt.1886" => "BT.1886".to_owned(),
        "srgb" => "sRGB".to_owned(),
        "gamma2.2" => "Gamma 2.2".to_owned(),
        "gamma2.8" => "Gamma 2.8".to_owned(),
        "unknown" => "Unknown".to_owned(),
        other => other.to_owned(),
    }
}

fn friendly_primaries(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "bt.2020" | "bt2020" => "BT.2020".to_owned(),
        "bt.709" | "bt709" => "BT.709".to_owned(),
        "bt.601-625" => "BT.601 PAL".to_owned(),
        "bt.601-525" => "BT.601 NTSC".to_owned(),
        "dci-p3" => "DCI-P3".to_owned(),
        "display-p3" => "Display P3".to_owned(),
        "unknown" => "Unknown".to_owned(),
        other => other.to_owned(),
    }
}

fn friendly_color_matrix(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "bt.2020-ncl" => "BT.2020 non-constant luminance".to_owned(),
        "bt.2020-cl" => "BT.2020 constant luminance".to_owned(),
        "bt.709" | "bt709" => "BT.709".to_owned(),
        "bt.601" | "bt601" => "BT.601".to_owned(),
        "smpte-240m" => "SMPTE 240M".to_owned(),
        "rgb" => "RGB".to_owned(),
        "unknown" => "Unknown".to_owned(),
        other => other.to_owned(),
    }
}

fn friendly_color_levels(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "limited" | "tv" => "Limited / TV".to_owned(),
        "full" | "pc" => "Full / PC".to_owned(),
        "unknown" => "Unknown".to_owned(),
        other => other.to_owned(),
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

fn normalized_video_rotation(degrees: i64) -> i64 {
    degrees.rem_euclid(360) / 90 * 90
}

fn video_aspect_override(value: &str) -> &str {
    match value {
        "16:9" => "16:9",
        "4:3" => "4:3",
        "2.35:1" => "2.35:1",
        _ => "no",
    }
}

fn parse_ab_loop_point(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "no" {
        return None;
    }

    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
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
        // Stop the pump before the handle is destroyed: it unsets the wakeup
        // callback and joins the background thread so no event reception or
        // property read can race `mpv_terminate_destroy`.
        if let Some(pump) = self.pump.take() {
            pump.shutdown();
        }
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

pub(crate) fn end_file_reason(reason: c_int, error: c_int) -> EndFileReason {
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

fn normalized_audio_device_name(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() {
        AUDIO_DEVICE_AUTO
    } else {
        name
    }
}

fn audio_device_selected(name: &str, current: &str) -> bool {
    name == normalized_audio_device_name(current)
}

fn audio_device_label(name: &str, description: Option<String>) -> String {
    let description = description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());
    if name == AUDIO_DEVICE_AUTO {
        description.unwrap_or("Automatic").to_owned()
    } else {
        description.unwrap_or(name).to_owned()
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use okp_test_fixtures::unique_temp_dir;

    use super::*;

    #[test]
    fn render_context_parameters_include_wayland_display_only_when_present() {
        let mut init_params = ffi::mpv_opengl_init_params {
            get_proc_address: None,
            get_proc_address_ctx: ptr::null_mut(),
        };
        let without_wayland = render_context_parameters(c"opengl", &mut init_params, None);
        assert_eq!(
            without_wayland
                .iter()
                .map(|parameter| parameter.param_type)
                .collect::<Vec<_>>(),
            vec![
                ffi::MPV_RENDER_PARAM_API_TYPE,
                ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                ffi::MPV_RENDER_PARAM_INVALID,
            ]
        );

        let display = NonNull::<c_void>::dangling();
        let with_wayland = render_context_parameters(c"opengl", &mut init_params, Some(display));
        assert_eq!(
            with_wayland
                .iter()
                .map(|parameter| parameter.param_type)
                .collect::<Vec<_>>(),
            vec![
                ffi::MPV_RENDER_PARAM_API_TYPE,
                ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                ffi::MPV_RENDER_PARAM_WL_DISPLAY,
                ffi::MPV_RENDER_PARAM_INVALID,
            ]
        );
        assert_eq!(with_wayland[2].data, display.as_ptr());
        assert!(with_wayland[3].data.is_null());
    }

    #[test]
    fn native_wayland_display_retains_its_owner_until_drop() {
        struct Owner(Rc<Cell<bool>>);

        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let dropped = Rc::new(Cell::new(false));
        let display = unsafe {
            NativeWaylandDisplay::new(NonNull::<c_void>::dangling(), Owner(Rc::clone(&dropped)))
        };
        assert!(!dropped.get());
        drop(display);
        assert!(dropped.get());
    }

    fn fixture_media_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv")
    }

    fn assert_same_stem_sidecar_autoloaded(extension: &str, contents: &str, expected_codec: &str) {
        let root = unique_temp_dir(&format!("okp-mpv-{extension}-autoload"));
        fs::create_dir_all(&root).expect("sidecar fixture directory should be created");
        let media = root.join("movie.mkv");
        fs::copy(fixture_media_path(), &media).expect("media fixture should be copied");
        fs::write(root.join(format!("movie.{extension}")), contents)
            .expect("subtitle sidecar should be written");

        let options = [
            ("vo".to_owned(), "null".to_owned()),
            ("ao".to_owned(), "null".to_owned()),
        ];
        let mut mpv = Mpv::new_with_options("no", &options)
            .expect("libmpv must be loadable for okp-mpv tests");
        mpv.start_event_pump();
        mpv.load_file(&media).expect("media fixture should load");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut tracks = Vec::new();
        while Instant::now() < deadline {
            tracks = mpv.observed_tracks();
            if tracks.iter().any(|track| {
                track.kind == TrackKind::Subtitle
                    && track.external
                    && track.codec.as_deref() == Some(expected_codec)
            }) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(
            tracks.iter().any(|track| {
                track.kind == TrackKind::Subtitle
                    && track.external
                    && track.codec.as_deref() == Some(expected_codec)
            }),
            "expected same-stem .{extension} sidecar as external {expected_codec} track, got {tracks:?}"
        );

        drop(mpv);
        fs::remove_dir_all(root).expect("sidecar fixture directory should be removed");
    }

    #[test]
    fn exact_same_stem_srt_sidecar_is_autoloaded() {
        assert_same_stem_sidecar_autoloaded(
            "srt",
            "1\n00:00:00,000 --> 00:00:02,000\nSRT SIDECAR\n",
            "subrip",
        );
    }

    #[test]
    fn exact_same_stem_webvtt_sidecar_is_autoloaded() {
        assert_same_stem_sidecar_autoloaded(
            "vtt",
            "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nWEBVTT SIDECAR\n",
            "webvtt",
        );
    }

    /// Real-libmpv twin of the Windows `MpvThreadGuardTests`: blocking reads
    /// on the marked UI thread must trip the debug guard, everything else must
    /// stay clean. Loads the actual engine, so it needs libmpv at test time —
    /// same contract as CI, which installs libmpv-dev before `cargo test`.
    #[test]
    #[cfg(debug_assertions)]
    fn blocking_reads_on_the_marked_ui_thread_trip_the_guard() {
        let mpv = Mpv::new().expect("libmpv must be loadable for okp-mpv tests");

        let _ = mpv
            .playback_state()
            .expect("playback state must be readable");
        assert_eq!(
            mpv.blocking_read_violations(),
            0,
            "reads before mark_ui_thread must not be flagged"
        );

        mpv.mark_ui_thread();
        let _ = mpv
            .playback_state()
            .expect("playback state must be readable");
        assert!(
            mpv.blocking_read_violations() > 0,
            "blocking reads on the marked UI thread must be recorded"
        );
    }

    /// The event pump must start, publish observed state off the UI thread, and
    /// tear down cleanly on drop without racing `mpv_terminate_destroy`. Needs
    /// real libmpv (same contract as the guard test). Observed reads never trip
    /// the guard even after the UI thread is marked, because they never call mpv.
    #[test]
    fn event_pump_publishes_observed_state_and_shuts_down_cleanly() {
        let mut mpv = Mpv::new().expect("libmpv must be loadable for okp-mpv tests");
        mpv.mark_ui_thread();
        mpv.start_event_pump();

        // Give the pump a moment to observe the initial property values.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Idle defaults: nothing playing, so scale/speed report their 1.0
        // fallbacks and the transport scalars are absent — all read from the
        // snapshot without touching mpv.
        assert_eq!(mpv.observed_subtitle_scale(), 1.0);
        assert_eq!(mpv.observed_speed(), 1.0);
        assert!(mpv.observed_playback_state().time_pos.is_none());
        let _ = mpv.observed_tracks();
        let _ = mpv.take_lifecycle_events();

        #[cfg(debug_assertions)]
        assert_eq!(
            mpv.blocking_read_violations(),
            0,
            "observed reads must never issue a blocking mpv read on the UI thread"
        );

        // Dropping here exercises pump shutdown + terminate ordering.
    }

    #[test]
    fn lifecycle_events_carry_display_dimensions_from_the_pump_thread() {
        let options = [
            ("vo".to_owned(), "null".to_owned()),
            ("ao".to_owned(), "null".to_owned()),
        ];
        let mut mpv = Mpv::new_with_options("no", &options)
            .expect("libmpv must be loadable for okp-mpv tests");
        mpv.mark_ui_thread();
        mpv.start_event_pump();
        mpv.load_file(&fixture_media_path())
            .expect("video fixture should load");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut dimensions = None;
        while Instant::now() < deadline && dimensions.is_none() {
            for event in mpv.take_lifecycle_events() {
                match event {
                    MpvEvent::FileLoaded { video_dimensions }
                    | MpvEvent::VideoReconfig { video_dimensions } => {
                        dimensions = dimensions.or(video_dimensions);
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            dimensions,
            Some(VideoDimensions {
                width: 1280,
                height: 720
            })
        );
        #[cfg(debug_assertions)]
        assert_eq!(
            mpv.blocking_read_violations(),
            0,
            "event payload generation must stay off the marked UI thread"
        );
    }

    /// Recording a media source after the `FileLoaded` recompute has already
    /// run must still refresh `media_info`: `set_media_source` has to wake the
    /// pump and rebuild the snapshot against the new path instead of waiting for
    /// an unrelated `track-list`/`chapter-list` change. Needs real libmpv.
    #[test]
    fn setting_the_media_source_refreshes_media_info() {
        let mut mpv = Mpv::new().expect("libmpv must be loadable for okp-mpv tests");
        mpv.mark_ui_thread();
        mpv.start_event_pump();

        // Let the initial recompute settle; with no source recorded yet it
        // builds `media_info` with no local path.
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            mpv.observed_media_info().and_then(|info| info.path),
            None,
            "media_info must have no path before a source is recorded"
        );

        let source = PathBuf::from("/tmp/okp-media-source-refresh.mkv");
        mpv.set_media_source(Some(source.clone()));

        // The setter wakes the pump, so the snapshot rebuilds without any
        // further mpv event. Poll for the rebuild instead of asserting after a
        // single fixed delay: the cross-thread wake can take longer than 100 ms
        // on a loaded CI runner, which made the fixed-sleep assertion flaky.
        let want = Some(source.display().to_string());
        let mut observed = None;
        for _ in 0..200 {
            observed = mpv.observed_media_info().and_then(|info| info.path);
            if observed == want {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            observed, want,
            "set_media_source must wake the pump and rebuild media_info"
        );

        #[cfg(debug_assertions)]
        assert_eq!(
            mpv.blocking_read_violations(),
            0,
            "observed reads must never issue a blocking mpv read on the UI thread"
        );
    }

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
    fn normalizes_video_transform_values() {
        assert_eq!(normalized_video_rotation(0), 0);
        assert_eq!(normalized_video_rotation(90), 90);
        assert_eq!(normalized_video_rotation(450), 90);
        assert_eq!(normalized_video_rotation(-90), 270);
        assert_eq!(video_aspect_override("16:9"), "16:9");
        assert_eq!(video_aspect_override("2.35:1"), "2.35:1");
        assert_eq!(video_aspect_override("-1"), "no");
    }

    #[test]
    fn parses_ab_loop_points() {
        assert_eq!(parse_ab_loop_point("no"), None);
        assert_eq!(parse_ab_loop_point(""), None);
        assert_eq!(parse_ab_loop_point("-1"), None);
        assert_eq!(parse_ab_loop_point("12.5"), Some(12.5));
        assert_eq!(parse_ab_loop_point("nan"), None);
    }

    #[test]
    fn audio_normalization_filter_is_labelled() {
        assert_eq!(AUDIO_NORMALIZATION_FILTER_LABEL, "@okpnorm");
        assert_eq!(AUDIO_NORMALIZATION_FILTER, "@okpnorm:dynaudnorm");
    }

    #[test]
    fn screenshot_modes_keep_clean_and_subtitled_captures_distinct() {
        assert_eq!(screenshot_mode(false), "video");
        assert_eq!(screenshot_mode(true), "subtitles");
    }

    #[test]
    fn normalizes_audio_device_names() {
        assert_eq!(normalized_audio_device_name(""), "auto");
        assert_eq!(normalized_audio_device_name("  "), "auto");
        assert_eq!(
            normalized_audio_device_name("pulse/alsa_output"),
            "pulse/alsa_output"
        );
    }

    #[test]
    fn formats_audio_device_labels() {
        assert_eq!(audio_device_label("auto", None), "Automatic");
        assert_eq!(
            audio_device_label("auto", Some("System default".to_owned())),
            "System default"
        );
        assert_eq!(
            audio_device_label("pulse/device", Some(" Speakers ".to_owned())),
            "Speakers"
        );
        assert_eq!(audio_device_label("pulse/device", None), "pulse/device");
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

    #[test]
    fn extracts_bit_depth_from_common_pixel_formats() {
        assert_eq!(bit_depth_from_pixel_format("yuv420p"), Some(8));
        assert_eq!(bit_depth_from_pixel_format("nv12"), Some(8));
        assert_eq!(bit_depth_from_pixel_format("p010"), Some(10));
        assert_eq!(bit_depth_from_pixel_format("yuv420p10"), Some(10));
        assert_eq!(bit_depth_from_pixel_format("yuv420p10le"), Some(10));
        assert_eq!(bit_depth_from_pixel_format("yuv444p12le"), Some(12));
        assert_eq!(bit_depth_from_pixel_format("p016"), Some(16));
        assert_eq!(bit_depth_from_pixel_format("vaapi"), None);
    }

    #[test]
    fn summarizes_dynamic_range_from_hdr_metadata() {
        assert_eq!(
            dynamic_range_summary(Some("pq"), Some("bt.2020"), Some(10.0), Some(1000.0)),
            Some("HDR (PQ / ST 2084, BT.2020)".to_owned())
        );
        assert_eq!(
            dynamic_range_summary(Some("hlg"), Some("bt.2020"), None, None),
            Some("HDR (HLG, BT.2020)".to_owned())
        );
        assert_eq!(
            dynamic_range_summary(Some("bt.1886"), Some("bt.709"), Some(1.0), Some(100.0)),
            Some("SDR".to_owned())
        );
        assert_eq!(dynamic_range_summary(None, None, None, None), None);
    }

    #[test]
    fn formats_color_metadata_for_media_info() {
        assert_eq!(friendly_transfer("pq"), "PQ / ST 2084");
        assert_eq!(friendly_primaries("bt.2020"), "BT.2020");
        assert_eq!(
            friendly_color_matrix("bt.2020-ncl"),
            "BT.2020 non-constant luminance"
        );
        assert_eq!(friendly_color_levels("limited"), "Limited / TV");
    }
}
