//! The playlist's filesystem work: listing a folder for its media, and turning an
//! open of folders and playlist files into the items to play.
//!
//! None of it runs on a caller's thread. A slow or sleeping network mount can
//! keep a directory read waiting for a long time, so every job runs on a thread
//! of its own ([`spawn`]), reads at most [`FOLDER_SCAN_LIMIT`] entries of a
//! directory, stops between entries once its cancel flag is set, and reports
//! through an inbox that the session reads on its event thread.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use okp_core::playlist::PlaylistItem;
use okp_core::{m3u, media_formats, natural_compare};

/// The most directory entries one listing reads. A folder with more entries is
/// left unlisted: the opened file stays alone in Up Next, and an opened folder
/// contributes nothing.
pub const FOLDER_SCAN_LIMIT: usize = 20_000;

/// The filesystem as the playlist sees it; a seam so that tests can make a
/// directory slow, huge or unreadable.
pub(crate) trait Filesystem: Send + Sync {
    /// The entries of `dir`, as full paths, read one at a time.
    fn read_dir(&self, dir: &Path) -> io::Result<Box<dyn Iterator<Item = PathBuf> + '_>>;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
}

/// The host's filesystem.
pub(crate) struct HostFilesystem;

impl Filesystem for HostFilesystem {
    fn read_dir(&self, dir: &Path) -> io::Result<Box<dyn Iterator<Item = PathBuf> + '_>> {
        Ok(Box::new(
            std::fs::read_dir(dir)?
                .filter_map(Result::ok)
                .map(|entry| entry.path()),
        ))
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// Why a directory listing produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListingStopped {
    /// A newer open superseded the job.
    Cancelled,
    /// The directory has more than [`FOLDER_SCAN_LIMIT`] entries.
    TooLarge,
    /// The directory could not be read.
    Unreadable,
}

/// The media files of `dir`, natural-sorted by file name (the GTK shell's
/// `media_paths_in_directory`).
pub(crate) fn media_in_directory(
    fs: &dyn Filesystem,
    dir: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<PathBuf>, ListingStopped> {
    let entries = fs.read_dir(dir).map_err(|_| ListingStopped::Unreadable)?;
    let mut files = Vec::new();
    for (count, path) in entries.enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err(ListingStopped::Cancelled);
        }
        if count >= FOLDER_SCAN_LIMIT {
            return Err(ListingStopped::TooLarge);
        }
        if media_formats::is_media(&path) {
            files.push(path);
        }
    }
    files.sort_by(|left, right| {
        let left = left.file_name().and_then(|name| name.to_str());
        let right = right.file_name().and_then(|name| name.to_str());
        natural_compare::compare(left, right)
    });
    Ok(files)
}

/// The folder playlist of `file`: the media of its directory in natural order,
/// with `file` first if the listing does not include it (the GTK shell's
/// `build_folder_playlist` and `remember_loaded_media_with_playlist`).
pub(crate) fn folder_playlist(file: &Path, siblings: Vec<PathBuf>) -> Vec<PlaylistItem> {
    let mut items: Vec<PlaylistItem> = siblings.into_iter().map(PlaylistItem::Local).collect();
    let anchor = PlaylistItem::Local(file.to_path_buf());
    if !items.contains(&anchor) {
        items.insert(0, anchor);
    }
    items
}

/// Whether `path` names an M3U playlist by its extension.
pub(crate) fn is_playlist_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("m3u") || extension.eq_ignore_ascii_case("m3u8")
        })
}

/// The playable entries of an M3U document, in document order, with relative
/// entries resolved against the playlist file's directory.
pub(crate) fn m3u_items(text: &str, playlist_file: &Path) -> Vec<PlaylistItem> {
    m3u::parse(text, playlist_file.parent())
        .iter()
        .filter_map(|entry| PlaylistItem::from_m3u_entry(entry))
        .collect()
}

/// What an open of several sources plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// One local media file: play it, with its folder as the playlist.
    Folder(PathBuf),
    /// These items, in this order, starting with the first.
    List(Vec<PlaylistItem>),
    /// Nothing playable was found.
    Nothing,
}

/// How an open's sources sort before any filesystem access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Classified {
    /// Known without touching the filesystem.
    Resolved(Resolution),
    /// A folder or a playlist file has to be read first.
    NeedsFilesystem,
}

/// Sort an open's sources by name alone: media files and URLs play, subtitles
/// are not media, and anything else may be a folder. Only when a source may be
/// a folder, or the only candidates are playlist files, does the open need the
/// filesystem.
pub(crate) fn classify(sources: &[PlaylistItem]) -> Classified {
    let mut media = Vec::new();
    let mut playlist_file = false;
    for source in sources {
        match source {
            PlaylistItem::Url(_) => media.push(source.clone()),
            PlaylistItem::Local(path) if media_formats::is_media(path) => {
                media.push(source.clone());
            }
            PlaylistItem::Local(path) if media_formats::is_subtitle(path) => {}
            PlaylistItem::Local(path) if is_playlist_file(path) => playlist_file = true,
            PlaylistItem::Local(_) => return Classified::NeedsFilesystem,
        }
    }
    if media.is_empty() && playlist_file {
        return Classified::NeedsFilesystem;
    }
    Classified::Resolved(resolution_of(media))
}

/// The resolution of an open's sources (the GTK shell's
/// `load_selected_local_paths` with `selected_media_paths`): folders expand to
/// their media in natural order, repeats are dropped and the given order is
/// kept. One local media file opens as a folder playlist and several as a list.
/// Only without any media does a playlist file count: the first one's entries.
pub(crate) fn resolve(
    fs: &dyn Filesystem,
    sources: &[PlaylistItem],
    cancel: &AtomicBool,
) -> Result<Resolution, ListingStopped> {
    let mut media = Vec::new();
    let mut playlist_file = None;
    for source in sources {
        if cancel.load(Ordering::Acquire) {
            return Err(ListingStopped::Cancelled);
        }
        match source {
            PlaylistItem::Url(_) => media.push(source.clone()),
            PlaylistItem::Local(path) if media_formats::is_media(path) => {
                media.push(source.clone());
            }
            PlaylistItem::Local(path) if media_formats::is_subtitle(path) => {}
            PlaylistItem::Local(path) if is_playlist_file(path) => {
                playlist_file.get_or_insert(path.as_path());
            }
            PlaylistItem::Local(path) if fs.is_dir(path) => {
                match media_in_directory(fs, path, cancel) {
                    Ok(files) => media.extend(files.into_iter().map(PlaylistItem::Local)),
                    Err(ListingStopped::Cancelled) => return Err(ListingStopped::Cancelled),
                    Err(ListingStopped::TooLarge | ListingStopped::Unreadable) => {}
                }
            }
            PlaylistItem::Local(_) => {}
        }
    }
    if media.is_empty()
        && let Some(path) = playlist_file
    {
        let items = fs
            .read_to_string(path)
            .map(|text| m3u_items(&text, path))
            .unwrap_or_default();
        return Ok(if items.is_empty() {
            Resolution::Nothing
        } else {
            Resolution::List(items)
        });
    }
    Ok(resolution_of(media))
}

fn resolution_of(media: Vec<PlaylistItem>) -> Resolution {
    let mut unique: Vec<PlaylistItem> = Vec::with_capacity(media.len());
    for item in media {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    match unique.as_slice() {
        [] => Resolution::Nothing,
        [PlaylistItem::Local(path)] => Resolution::Folder(path.clone()),
        _ => Resolution::List(unique),
    }
}

/// Work for a scan thread.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    /// List the folder of this file.
    Folder(PathBuf),
    /// Resolve these open sources.
    Resolve(Vec<PlaylistItem>),
}

/// What a scan thread found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The media of `file`'s folder, or `None` when it could not be listed.
    Folder {
        file: PathBuf,
        siblings: Option<Vec<PathBuf>>,
    },
    Resolved(Resolution),
}

/// A finished job, tagged with the id it was started under.
#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) job: u64,
    pub(crate) outcome: Outcome,
}

/// Called from a scan thread when a job has finished.
pub(crate) type Notify = Arc<dyn Fn() + Send + Sync>;

/// Where scan threads leave their results.
pub(crate) type Inbox = Arc<Mutex<Vec<Finished>>>;

/// Run `job` on a new thread. A thread is never joined: one stuck in a read of
/// a hung mount is abandoned, and whatever it reports later is ignored by id. A
/// cancelled job reports nothing. Returns false if no thread could be started.
pub(crate) fn spawn(
    fs: Arc<dyn Filesystem>,
    job: Job,
    id: u64,
    cancel: Arc<AtomicBool>,
    inbox: Inbox,
    notify: Notify,
) -> bool {
    thread::Builder::new()
        .name("okp-playlist-scan".to_owned())
        .spawn(move || {
            let outcome = match job {
                Job::Folder(file) => {
                    let siblings = match file.parent() {
                        Some(dir) => media_in_directory(&*fs, dir, &cancel),
                        None => Err(ListingStopped::Unreadable),
                    };
                    if siblings == Err(ListingStopped::Cancelled) {
                        return;
                    }
                    Outcome::Folder {
                        file,
                        siblings: siblings.ok(),
                    }
                }
                Job::Resolve(sources) => match resolve(&*fs, &sources, &cancel) {
                    Ok(resolution) => Outcome::Resolved(resolution),
                    Err(_) => return,
                },
            };
            inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(Finished { job: id, outcome });
            notify();
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;
    use std::fs;

    fn local(path: &str) -> PlaylistItem {
        PlaylistItem::Local(PathBuf::from(path))
    }

    fn not_cancelled() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// A directory of `count` generated entries that yields them on demand.
    struct HugeDirectory {
        count: usize,
    }

    impl Filesystem for HugeDirectory {
        fn read_dir(&self, dir: &Path) -> io::Result<Box<dyn Iterator<Item = PathBuf> + '_>> {
            let dir = dir.to_path_buf();
            Ok(Box::new(
                (0..self.count).map(move |index| dir.join(format!("{index}.mkv"))),
            ))
        }

        fn is_dir(&self, _: &Path) -> bool {
            true
        }

        fn read_to_string(&self, _: &Path) -> io::Result<String> {
            Err(io::ErrorKind::NotFound.into())
        }
    }

    #[test]
    fn a_folder_lists_its_media_in_natural_order_by_file_name() {
        let root = unique_temp_dir("okp-session-folder");
        for name in [
            "Episode 10.mkv",
            "Episode 2.MP4",
            "Episode 1.mkv",
            "Episode 2.srt",
            "cover.jpg",
            "list.m3u",
        ] {
            fs::write(root.path().join(name), []).unwrap();
        }

        let files = media_in_directory(&HostFilesystem, root.path(), &not_cancelled()).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Episode 1.mkv", "Episode 2.MP4", "Episode 10.mkv"]);
    }

    #[test]
    fn a_folder_playlist_puts_an_unlisted_file_first_and_otherwise_keeps_the_order() {
        let listed = vec![PathBuf::from("/m/a.mkv"), PathBuf::from("/m/b.mkv")];
        assert_eq!(
            folder_playlist(Path::new("/m/b.mkv"), listed.clone()),
            [local("/m/a.mkv"), local("/m/b.mkv")]
        );
        assert_eq!(
            folder_playlist(Path::new("/m/c.mkv"), listed),
            [local("/m/c.mkv"), local("/m/a.mkv"), local("/m/b.mkv")]
        );
        assert_eq!(
            folder_playlist(Path::new("/m/c.mkv"), Vec::new()),
            [local("/m/c.mkv")]
        );
    }

    #[test]
    fn a_listing_reads_at_most_the_limit_and_stops_when_cancelled() {
        let at_limit = HugeDirectory {
            count: FOLDER_SCAN_LIMIT,
        };
        assert_eq!(
            media_in_directory(&at_limit, Path::new("/big"), &not_cancelled())
                .map(|files| files.len()),
            Ok(FOLDER_SCAN_LIMIT)
        );
        let over = HugeDirectory {
            count: FOLDER_SCAN_LIMIT + 1,
        };
        assert_eq!(
            media_in_directory(&over, Path::new("/big"), &not_cancelled()),
            Err(ListingStopped::TooLarge)
        );
        assert_eq!(
            media_in_directory(&at_limit, Path::new("/big"), &AtomicBool::new(true)),
            Err(ListingStopped::Cancelled)
        );
        assert_eq!(
            media_in_directory(
                &HostFilesystem,
                Path::new("/nonexistent-okp-folder"),
                &not_cancelled()
            ),
            Err(ListingStopped::Unreadable)
        );
    }

    #[test]
    fn playlist_files_are_recognised_by_extension() {
        assert!(is_playlist_file(Path::new("/tmp/list.m3u")));
        assert!(is_playlist_file(Path::new("/tmp/list.M3U8")));
        assert!(!is_playlist_file(Path::new("/tmp/movie.mkv")));
        assert!(!is_playlist_file(Path::new("/tmp/m3u")));
    }

    #[test]
    fn m3u_entries_keep_urls_skip_non_media_and_resolve_against_the_playlist_file() {
        let text = "#EXTM3U\n/media/ep2.mkv\nhttps://example.test/ep3.mp4\n\
                    /media/captions.srt\n/media/readme.txt\nep1.mp4\n../other/ep4.mkv\n";
        let items = m3u_items(text, Path::new("/lists/shows/queue.m3u8"));
        assert_eq!(
            items,
            [
                local("/media/ep2.mkv"),
                PlaylistItem::Url("https://example.test/ep3.mp4".to_owned()),
                local("/lists/shows/ep1.mp4"),
                local("/lists/shows/../other/ep4.mkv"),
            ]
        );
    }

    #[test]
    fn a_selection_of_media_keeps_its_order_and_drops_repeats_without_the_filesystem() {
        // Subtitles and the playlist file are not media; with media present the
        // playlist file is ignored, so nothing needs the filesystem.
        let sources = [
            local("/media/b.mkv"),
            local("/media/subs.srt"),
            local("/media/a.mp4"),
            local("/media/b.mkv"),
            local("/media/list.m3u"),
        ];
        assert_eq!(
            classify(&sources),
            Classified::Resolved(Resolution::List(vec![
                local("/media/b.mkv"),
                local("/media/a.mp4")
            ]))
        );
        assert_eq!(
            classify(&[local("/media/b.mkv"), local("/media/b.mkv")]),
            Classified::Resolved(Resolution::Folder(PathBuf::from("/media/b.mkv")))
        );
        let url = PlaylistItem::Url("https://example.test/a.mp4".to_owned());
        assert_eq!(
            classify(std::slice::from_ref(&url)),
            Classified::Resolved(Resolution::List(vec![url]))
        );
        assert_eq!(
            classify(&[local("/media/subs.srt")]),
            Classified::Resolved(Resolution::Nothing)
        );
        assert_eq!(
            classify(&[local("/media/list.m3u")]),
            Classified::NeedsFilesystem
        );
        assert_eq!(
            classify(&[local("/media/Season 1"), local("/media/a.mkv")]),
            Classified::NeedsFilesystem
        );
    }

    #[test]
    fn folders_expand_in_natural_order_and_in_selection_order() {
        let root = unique_temp_dir("okp-session-folder-selection");
        let season_one = root.path().join("Season 1");
        let season_two = root.path().join("Season 2");
        fs::create_dir_all(&season_one).unwrap();
        fs::create_dir_all(&season_two).unwrap();
        let s1e1 = season_one.join("Episode 1.mkv");
        let s1e2 = season_one.join("Episode 2.mkv");
        let s2e1 = season_two.join("Episode 1.mkv");
        let s2e10 = season_two.join("Episode 10.mkv");
        for path in [&s1e2, &s1e1, &s2e10, &s2e1] {
            fs::write(path, []).unwrap();
        }
        fs::write(season_two.join("Episode 1.srt"), []).unwrap();

        let sources = [
            PlaylistItem::Local(season_two),
            PlaylistItem::Local(season_one.clone()),
            PlaylistItem::Local(s1e1.clone()),
        ];
        assert_eq!(
            resolve(&HostFilesystem, &sources, &not_cancelled()),
            Ok(Resolution::List(
                [s2e1, s2e10, s1e1.clone(), s1e2]
                    .into_iter()
                    .map(PlaylistItem::Local)
                    .collect()
            ))
        );

        // A folder with one media file opens it as a folder playlist.
        let single = root.path().join("Single");
        fs::create_dir(&single).unwrap();
        fs::write(single.join("only.mkv"), []).unwrap();
        assert_eq!(
            resolve(
                &HostFilesystem,
                &[PlaylistItem::Local(single.clone())],
                &not_cancelled()
            ),
            Ok(Resolution::Folder(single.join("only.mkv")))
        );
    }

    #[test]
    fn a_playlist_file_counts_only_without_media_and_reads_relative_entries() {
        let root = unique_temp_dir("okp-session-m3u");
        let list = root.path().join("queue.m3u");
        fs::write(&list, "#EXTM3U\nb.mkv\nnotes.txt\na.mkv\nb.mkv\n").unwrap();
        let empty = root.path().join("empty.m3u8");
        fs::write(&empty, "#EXTM3U\nnotes.txt\n").unwrap();

        // Entries in document order, repeats kept, resolved against the folder.
        let resolved = resolve(
            &HostFilesystem,
            &[PlaylistItem::Local(list.clone())],
            &not_cancelled(),
        );
        let entry =
            |name: &str| PlaylistItem::Local(std::path::absolute(root.path().join(name)).unwrap());
        assert_eq!(
            resolved,
            Ok(Resolution::List(vec![
                entry("b.mkv"),
                entry("a.mkv"),
                entry("b.mkv")
            ]))
        );
        assert_eq!(
            resolve(
                &HostFilesystem,
                &[PlaylistItem::Local(empty), PlaylistItem::Local(list)],
                &not_cancelled()
            ),
            Ok(Resolution::Nothing),
            "only the first playlist file is read"
        );
        assert_eq!(
            resolve(
                &HostFilesystem,
                &[
                    PlaylistItem::Local(root.path().join("missing.m3u")),
                    PlaylistItem::Local(root.path().join("notes.txt")),
                ],
                &not_cancelled()
            ),
            Ok(Resolution::Nothing)
        );
    }

    #[test]
    fn a_cancelled_resolution_reports_nothing() {
        assert_eq!(
            resolve(
                &HugeDirectory { count: 10 },
                &[local("/big")],
                &AtomicBool::new(true)
            ),
            Err(ListingStopped::Cancelled)
        );
    }
}
