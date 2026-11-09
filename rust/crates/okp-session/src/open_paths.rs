//! What a shell opens: the file types its open panel offers, and what a set of
//! local paths does when they arrive from an open panel, a drop onto the window
//! or the system's open events (Finder's Open With, a drop on the Dock icon).
//!
//! Both answers use okp-core's rules (`media_formats`) and the playlist's, so a
//! shell keeps no extension lists of its own. Neither touches the filesystem:
//! which paths are folders is the shell's to say (it has the file URLs), and a
//! folder goes to the playlist, which reads it on a scan thread
//! (`playlist::scan`), so a sleeping network mount never holds a query.

use std::path::Path;

use okp_core::media_formats;
use serde::{Deserialize, Serialize};

use crate::playlist::scan::{PLAYLIST_EXTENSIONS, is_playlist_file};
use crate::protocol::{MediaSource, SourceKind, Status};

/// The file types an open panel offers, by kind: lowercase extensions without
/// the leading dot, in okp-core's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenFormats {
    pub video: Vec<String>,
    pub audio: Vec<String>,
    pub subtitle: Vec<String>,
    /// Playlist files: M3U and M3U8.
    pub playlist: Vec<String>,
}

impl OpenFormats {
    /// The types of `media_formats` and the playlist files the playlist reads.
    #[must_use]
    pub fn current() -> Self {
        fn bare(extensions: &[&str]) -> Vec<String> {
            extensions
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_owned())
                .collect()
        }
        Self {
            video: bare(media_formats::VIDEO_EXTENSIONS),
            audio: bare(media_formats::AUDIO_EXTENSIONS),
            subtitle: bare(media_formats::SUBTITLE_EXTENSIONS),
            playlist: bare(&PLAYLIST_EXTENSIONS),
        }
    }
}

/// What opening a set of local paths does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OpenPlan {
    /// Replace the playlist with these sources, in the given order, through the
    /// playlist's `open`: media files, folders and M3U files. Subtitle files
    /// and files of any other kind among them are left out.
    Open { sources: Vec<MediaSource> },
    /// Only subtitle files, while media is loaded: load them onto that media.
    AddSubtitles { paths: Vec<String> },
    /// Nothing to open: no media file, folder or M3U file, and no subtitle file
    /// to load onto playing media.
    Nothing {},
}

/// The plan for `paths` while the session is in `status` (the GTK shell's
/// `load_selected_local_paths`). A path opens when okp-core's rules name it a
/// media file, when it is an M3U file, or when the shell knows it is a folder
/// (`folders`, which the playlist lists). Anything else, such as a document or
/// an archive, is left out, so a drop of only such files opens nothing and the
/// media that plays goes on. Subtitle files alone are loaded onto media that is
/// playing or paused. Repeated paths count once.
#[must_use]
pub fn plan(paths: &[String], folders: &[String], status: Status) -> OpenPlan {
    let mut sources: Vec<MediaSource> = Vec::new();
    let mut subtitles: Vec<String> = Vec::new();
    for path in paths.iter().filter(|path| !path.is_empty()) {
        let file = Path::new(path);
        let folder = folders.contains(path);
        if !folder && media_formats::is_subtitle(file) {
            if !subtitles.contains(path) {
                subtitles.push(path.clone());
            }
            continue;
        }
        if !folder && !media_formats::is_media(file) && !is_playlist_file(file) {
            continue;
        }
        let source = MediaSource {
            kind: SourceKind::Local,
            location: path.clone(),
        };
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    if !sources.is_empty() {
        return OpenPlan::Open { sources };
    }
    if !subtitles.is_empty() && matches!(status, Status::Playing | Status::Paused) {
        return OpenPlan::AddSubtitles { paths: subtitles };
    }
    OpenPlan::Nothing {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn local(location: &str) -> MediaSource {
        MediaSource {
            kind: SourceKind::Local,
            location: location.to_owned(),
        }
    }

    #[test]
    fn several_files_open_as_one_playlist_in_the_given_order() {
        let plan = plan(
            &paths(&["/m/b.mkv", "/m/a.mp3", "/m/b.mkv"]),
            &[],
            Status::Idle,
        );
        assert_eq!(
            plan,
            OpenPlan::Open {
                sources: vec![local("/m/b.mkv"), local("/m/a.mp3")]
            }
        );
    }

    #[test]
    fn a_folder_and_a_playlist_file_go_to_the_playlist_unread() {
        // Neither is read here: the playlist lists the folder and reads the
        // M3U file on its scan thread.
        assert_eq!(
            plan(&paths(&["/m/Show"]), &paths(&["/m/Show"]), Status::Playing),
            OpenPlan::Open {
                sources: vec![local("/m/Show")]
            }
        );
        assert_eq!(
            plan(&paths(&["/m/queue.M3U8"]), &[], Status::Idle),
            OpenPlan::Open {
                sources: vec![local("/m/queue.M3U8")]
            }
        );
        // The shell's word decides a folder, whatever its name looks like.
        assert_eq!(
            plan(
                &paths(&["/m/Extras.srt"]),
                &paths(&["/m/Extras.srt"]),
                Status::Idle
            ),
            OpenPlan::Open {
                sources: vec![local("/m/Extras.srt")]
            }
        );
    }

    #[test]
    fn files_that_are_neither_media_nor_playlists_open_nothing() {
        // A document, an archive or a path the shell does not know as a folder
        // would replace what plays with an open that finds nothing.
        for status in [Status::Idle, Status::Playing] {
            assert_eq!(
                plan(&paths(&["/m/doc.pdf"]), &[], status),
                OpenPlan::Nothing {},
                "{status:?}"
            );
            assert_eq!(
                plan(
                    &paths(&["/m/doc.pdf", "/m/notes.zip", "/m/Show"]),
                    &[],
                    status
                ),
                OpenPlan::Nothing {},
                "{status:?}"
            );
        }
        // Next to media they are left out.
        assert_eq!(
            plan(&paths(&["/m/doc.pdf", "/m/film.mkv"]), &[], Status::Playing),
            OpenPlan::Open {
                sources: vec![local("/m/film.mkv")]
            }
        );
        // A subtitle next to them still loads onto playing media.
        assert_eq!(
            plan(&paths(&["/m/doc.pdf", "/m/film.srt"]), &[], Status::Playing),
            OpenPlan::AddSubtitles {
                paths: paths(&["/m/film.srt"])
            }
        );
    }

    #[test]
    fn subtitles_alone_load_onto_media_that_plays_or_is_paused() {
        let dropped = paths(&["/m/film.en.srt", "/m/film.ASS", "/m/film.en.srt"]);
        let expected = OpenPlan::AddSubtitles {
            paths: paths(&["/m/film.en.srt", "/m/film.ASS"]),
        };
        assert_eq!(plan(&dropped, &[], Status::Playing), expected);
        assert_eq!(plan(&dropped, &[], Status::Paused), expected);
    }

    #[test]
    fn subtitles_alone_do_nothing_without_loaded_media() {
        for status in [Status::Idle, Status::Opening, Status::Ended] {
            assert_eq!(
                plan(&paths(&["/m/film.srt"]), &[], status),
                OpenPlan::Nothing {},
                "{status:?}"
            );
        }
    }

    #[test]
    fn subtitles_next_to_media_are_left_out_of_the_open() {
        assert_eq!(
            plan(
                &paths(&["/m/film.srt", "/m/film.mkv"]),
                &[],
                Status::Playing
            ),
            OpenPlan::Open {
                sources: vec![local("/m/film.mkv")]
            }
        );
    }

    #[test]
    fn no_paths_do_nothing() {
        assert_eq!(plan(&[], &[], Status::Playing), OpenPlan::Nothing {});
        assert_eq!(
            plan(&paths(&[""]), &paths(&[""]), Status::Playing),
            OpenPlan::Nothing {}
        );
    }

    #[test]
    fn the_formats_are_the_core_lists_without_dots() {
        let formats = OpenFormats::current();
        assert_eq!(formats.video.len(), media_formats::VIDEO_EXTENSIONS.len());
        assert_eq!(formats.audio.len(), media_formats::AUDIO_EXTENSIONS.len());
        assert_eq!(
            formats.subtitle.len(),
            media_formats::SUBTITLE_EXTENSIONS.len()
        );
        for extension in ["mkv", "webm", "mp4"] {
            assert!(formats.video.iter().any(|known| known == extension));
        }
        for extension in ["mp3", "flac"] {
            assert!(formats.audio.iter().any(|known| known == extension));
        }
        assert!(formats.subtitle.iter().any(|known| known == "srt"));
        assert_eq!(formats.playlist, ["m3u", "m3u8"]);
        for (kind, list) in [
            ("video", &formats.video),
            ("audio", &formats.audio),
            ("subtitle", &formats.subtitle),
        ] {
            for extension in list {
                assert!(
                    !extension.starts_with('.') && *extension == extension.to_ascii_lowercase(),
                    "{kind} .{extension}"
                );
                // Each listed type is what the core rule recognises.
                let path = format!("/m/file.{extension}");
                let recognised = match kind {
                    "subtitle" => media_formats::is_subtitle(&path),
                    _ => media_formats::is_media(&path),
                };
                assert!(recognised, "{kind} .{extension}");
            }
        }
        for extension in &formats.playlist {
            assert!(is_playlist_file(Path::new(&format!("/m/list.{extension}"))));
        }
    }
}
