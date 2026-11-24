//! Headless frame sampling on a dedicated libmpv instance. The caller owns the
//! background job and the image cache; playback is never touched.
use std::path::Path;
use std::time::{Duration, Instant, UNIX_EPOCH};

use okp_core::image_luma::{DEFAULT_STRIDE, mean_bgra};
use okp_core::poster_frame::{
    PosterFrameScorer, PosterSource, PosterVerdict, classify_source, poster_cache_key,
    poster_sample_offsets,
};

use super::{Mpv, ffi};

pub const POSTER_WIDTH: usize = 384;
pub const POSTER_HEIGHT: usize = 216;

/// A fingerprint for a sampleable local video, including its current file revision.
pub fn history_poster_key(path: &str) -> Option<String> {
    if classify_source(path) != PosterSource::LocalVideo || !Path::new(path).is_absolute() {
        return None;
    }
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(poster_cache_key(
        path,
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos(),
    ))
}

/// Produce a 384×216 native-order RGBx frame, or keep the caller's fallback.
pub fn decode_history_poster(path: &str, duration: f64) -> Option<Vec<u8>> {
    history_poster_key(path)?;
    let options = [
        ("aid", "no"),
        ("sid", "no"),
        ("pause", "yes"),
        ("keep-open", "yes"),
        ("hr-seek", "yes"),
        ("panscan", "1"),
        ("osd-level", "0"),
        ("autoload-files", "no"),
        ("access-references", "no"),
        ("demuxer-lavf-o", "protocol_whitelist=file"),
    ]
    .map(|(name, value)| (name.to_owned(), value.to_owned()));
    // A poster job runs after the primary engine has set LC_NUMERIC.
    // Creating a worker decoder must avoid mutating the process locale.
    let mut decoder = Mpv::create_uninitialized_handle().ok()?;
    decoder
        .configure_before_initialize("no", "libmpv", &options)
        .ok()?;
    // The bundled macOS mpv has no scripting support and omits these options.
    // Disable them only on builds that expose them.
    for name in ["load-scripts", "ytdl"] {
        decoder.set_option_if_supported(name, "no").ok()?;
    }
    let mut decoder = decoder.initialize().ok()?;
    decoder.create_software_render_context().ok()?;
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut candidate = vec![0; POSTER_WIDTH * POSTER_HEIGHT * 4];
    let mut best = Vec::new();
    let mut scorer = PosterFrameScorer::new();
    let mut previous = None;
    for planned in poster_sample_offsets(duration) {
        // The three-second floor can exceed the duration of short clips.
        let offset = if duration.is_finite() && duration > 0.0 {
            planned.min(duration * 0.90)
        } else {
            planned
        };
        if previous == Some(offset) {
            continue;
        }
        previous = Some(offset);
        // A fresh load with an exact start gives each restarted decoder a single
        // frame boundary. Waiting on a timer alone can capture a preceding sample.
        if decoder
            .load_with_file_options(path, &[("start", offset.to_string())])
            .is_err()
            || !wait_for_poster_frame(&decoder, deadline)
        {
            break;
        }
        if decoder
            .render_software(
                POSTER_WIDTH as i32,
                POSTER_HEIGHT as i32,
                POSTER_WIDTH * 4,
                &mut candidate,
            )
            .is_err()
        {
            break;
        }
        #[cfg(target_endian = "big")]
        for pixel in candidate.chunks_exact_mut(4) {
            pixel.reverse();
        }
        scorer.observe(offset, mean_bgra(&candidate, DEFAULT_STRIDE));
        if matches!(scorer.verdict(), PosterVerdict::Usable { offset: winner, .. } if winner == offset)
        {
            best.clone_from(&candidate);
        }
        if scorer.is_satisfied() {
            break;
        }
    }
    (!best.is_empty()).then_some(best)
}

fn wait_for_poster_frame(decoder: &Mpv, deadline: Instant) -> bool {
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        // This instance has no event pump. libmpv owns the event until the next wait.
        let event =
            unsafe { &*ffi::mpv_wait_event(decoder.handle.as_ptr(), remaining.as_secs_f64()) };
        match event.event_id {
            ffi::MPV_EVENT_PLAYBACK_RESTART => return true,
            ffi::MPV_EVENT_END_FILE if event.error < 0 => return false,
            ffi::MPV_EVENT_NONE | ffi::MPV_EVENT_SHUTDOWN => return false,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;

    #[test]
    fn history_poster_decodes_real_video_and_keeps_black_video_as_fallback() {
        let root = unique_temp_dir("okp-history-poster");
        let fixture = |path: &str, luma: u8| {
            let path = root.path().join(path);
            let mut bytes = b"YUV4MPEG2 W32 H18 F10:1 Ip A1:1 C420jpeg\n".to_vec();
            let chroma = [128; 32 * 18 / 2];
            for _ in 0..80 {
                bytes.extend(b"FRAME\n");
                bytes.extend([luma; 32 * 18]);
                bytes.extend(chroma);
            }
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let bright = fixture("bright.y4m", 192);
        let black = fixture("black.y4m", 16);
        let pixels = decode_history_poster(bright.to_str().unwrap(), 8.0)
            .expect("the headless decoder should return a lit video frame");
        assert_eq!(pixels.len(), POSTER_WIDTH * POSTER_HEIGHT * 4);
        assert!(mean_bgra(&pixels, DEFAULT_STRIDE) > 100.0);
        assert!(decode_history_poster(black.to_str().unwrap(), 8.0).is_none());
    }
}
