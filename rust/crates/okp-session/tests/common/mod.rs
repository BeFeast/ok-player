//! Helpers shared by the tests that run a real libmpv session.

#![allow(dead_code)] // Each test binary uses a different subset.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use okp_core::app::AppIdentity;
use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{MpvOption, SessionConfig};

pub const CHAPTER_TITLES: [&str; 2] = ["Opening", "Credits"];

/// One real libmpv session at a time per test binary, so parallel test threads
/// never start and tear down engines against each other.
pub fn engine_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A headless session config rooted in `dir`: no audio or video output, and
/// paused on load so a short fixture cannot reach its end mid-test.
pub fn headless_config(dir: &Path) -> SessionConfig {
    SessionConfig {
        schema_version: SCHEMA_VERSION,
        app: AppIdentity {
            name: "OK Player".to_owned(),
            platform: okp_core::app::Platform::Linux,
        },
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        cache_dir: dir.join("cache"),
        hwdec: Some("no".to_owned()),
        mpv_options: [("vo", "null"), ("ao", "null"), ("pause", "yes")]
            .into_iter()
            .map(|(name, value)| MpvOption {
                name: name.to_owned(),
                value: value.to_owned(),
            })
            .collect(),
        shell_commands: Vec::new(),
    }
}

/// A real media file with one audio track and two titled chapters: an mpv EDL
/// that plays two one-second ranges of a generated WAV tone. mpv starts a
/// chapter at every EDL range and takes its title from the range.
pub fn write_media_fixture(dir: &Path) -> PathBuf {
    let tone = dir.join("tone.wav");
    write_tone(&tone, 2);
    let tone = tone.to_str().expect("temporary paths are UTF-8");
    let mut edl = String::from("# mpv EDL v0\n");
    for (index, title) in CHAPTER_TITLES.iter().enumerate() {
        // `%<bytes>%<path>` quotes the path whatever characters it contains.
        edl.push_str(&format!("%{}%{tone},{index},1,title={title}\n", tone.len()));
    }
    let media = dir.join("movie.edl");
    fs::write(&media, edl).expect("the EDL fixture should be written");
    media
}

/// A real file with two audio tracks and no chapters: an mpv EDL that plays two
/// generated WAV tones as parallel streams (`!new_stream`), so libmpv numbers
/// them audio 1 and audio 2.
pub fn write_two_audio_fixture(dir: &Path) -> PathBuf {
    let mut edl = String::from("# mpv EDL v0\n");
    for (index, name) in ["first.wav", "second.wav"].iter().enumerate() {
        let tone = dir.join(name);
        write_tone(&tone, 2);
        let tone = tone.to_str().expect("temporary paths are UTF-8");
        if index > 0 {
            edl.push_str("!new_stream\n");
        }
        edl.push_str(&format!("%{}%{tone},0,2\n", tone.len()));
    }
    let media = dir.join("two-audio.edl");
    fs::write(&media, edl).expect("the EDL fixture should be written");
    media
}

/// The WAV tone a media fixture plays: a second real file with one audio track.
pub fn tone_fixture(dir: &Path) -> PathBuf {
    dir.join("tone.wav")
}

/// 16-bit mono PCM at 8 kHz: a quiet 440 Hz tone.
fn write_tone(path: &Path, seconds: u32) {
    const RATE: u32 = 8_000;
    let samples = RATE * seconds;
    let data_len = samples * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for index in 0..samples {
        let phase = f64::from(index) * 440.0 * std::f64::consts::TAU / f64::from(RATE);
        bytes.extend_from_slice(&((phase.sin() * 4_000.0) as i16).to_le_bytes());
    }
    fs::write(path, bytes).expect("the WAV fixture should be written");
}

/// Poll `condition` until it holds or five seconds pass.
pub fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    condition()
}
