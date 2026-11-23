//! Add sidecars while mpv has the demuxer open and before it selects tracks.

use std::ffi::CString;
use std::path::Path;
use std::ptr::{self, NonNull};

use okp_core::subtitle_sidecars;

use crate::ffi;
use crate::player::{RawReader, TrackKind, path_to_cstring};

const PRELOADED: u64 = 1;

pub(crate) fn register(handle: NonNull<ffi::mpv_handle>) -> bool {
    let result =
        unsafe { ffi::mpv_hook_add(handle.as_ptr(), PRELOADED, c"on_preloaded".as_ptr(), 0) };
    if result < 0 {
        eprintln!("[okp-mpv] could not register sidecar subtitle hook ({result})");
    }
    result >= 0
}

pub(crate) fn handle_hook(reader: &RawReader, event: &ffi::mpv_event) {
    if let Some(hook) = unsafe { event.data.cast::<ffi::mpv_event_hook>().as_ref() } {
        if event.reply_userdata == PRELOADED {
            load(reader);
        }
        // mpv waits for every hook to continue, including when a sidecar fails.
        unsafe { ffi::mpv_hook_continue(reader.handle().as_ptr(), hook.id) };
    }
}

fn load(reader: &RawReader) {
    let Some(path) = reader.path() else { return };
    if path.contains("://")
        || reader.get_flag("options/autoload-files").ok().flatten() == Some(false)
        || reader
            .get_string("options/sub-auto")
            .ok()
            .flatten()
            .as_deref()
            != Some("no")
    {
        return;
    }
    let sidecars = subtitle_sidecars::discover(Path::new(&path));
    if sidecars.is_empty() {
        return;
    }
    let Some(sid) = reader.get_string("options/sid").ok().flatten() else {
        return;
    };
    let preferred = reader
        .get_string("options/slang")
        .ok()
        .flatten()
        .unwrap_or_default();
    // File-local sub-files may already have loaded the same path.
    let existing = reader.tracks().unwrap_or_default();
    for sidecar in sidecars {
        if existing.iter().any(|track| {
            track.kind == TrackKind::Subtitle
                && track.external_filename.as_deref().map(Path::new) == Some(sidecar.path.as_path())
        }) {
            continue;
        }
        let language = sidecar
            .language
            .map_or("und", |language| language.code(&preferred));
        let (Ok(path), Ok(title), Ok(language)) = (
            path_to_cstring(&sidecar.path),
            CString::new(sidecar.title),
            CString::new(language),
        ) else {
            continue;
        };
        // "auto" excludes the new track from mpv's usual external-track
        // fallback. "select" adds a normal track and only updates the sid
        // option at this stage, since playback has not been initialized.
        let args = [
            c"sub-add".as_ptr(),
            path.as_ptr(),
            c"select".as_ptr(),
            title.as_ptr(),
            language.as_ptr(),
            ptr::null(),
        ];
        unsafe { ffi::mpv_command(reader.handle().as_ptr(), args.as_ptr()) };
    }
    // Restore auto/Off/the requested ID before mpv selects tracks using slang.
    if let Err(error) = reader.set_string_property("options/sid", &sid) {
        eprintln!("[okp-mpv] could not restore subtitle selection: {error}");
    }
}
